//! Explicit positional retrieval: file-level posting admission followed by
//! verification of one captured source version, before owner grouping or top-k.
use super::{QueryEngine, Seeds, effective_limit, rank_score, result_truncations, select_diverse};
use crate::positional::PositionalQuery;
use crate::{Error, Result, config::WalkPolicy};
use graph_search_types::{
    ExploreMode, ExploreQuery, Node, NodeId, NodeKind, RetrievalEvidence, RetrievalPlan,
    RetrievalRoute, Scored, context::ResultContext,
};
use graph_search_types::{Span, source::SourceEvidence};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

impl QueryEngine<'_> {
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) fn seed_positional(
        &self,
        query: &ExploreQuery,
        root: &Path,
        policy: &WalkPolicy,
        sources: &mut crate::source::SourceCache,
        context: &ResultContext,
        predicate: &PositionalQuery,
    ) -> Result<Seeds> {
        let terms = predicate.candidate_terms();
        let k = if query.k == 0 {
            graph_search_types::limits::EXPLORE_DEFAULT_K
        } else {
            effective_limit(query.k)
        };
        let report = crate::walk::walk_report_with_work(
            &crate::walk::resolve_search_root(root, None)?,
            policy,
            &mut self.work.borrow_mut(),
        )?;
        let coverage = report.coverage;
        let mut truncations = coverage.truncations.clone();
        let changed: BTreeSet<_> = context.staleness.changed_paths.iter().cloned().collect();
        let unchecked = context.freshness
            == graph_search_types::context::FreshnessMethod::Unchecked
            || context.coverage.enumeration_complete != Some(true);
        let representation_changed = context
            .indexed_versions
            .is_some_and(|versions| !versions.retrieval_is_current());
        // When all eligible files need verification, postings cannot safely
        // exclude any file and would only spend the shared candidate allowance.
        let candidates: BTreeSet<_> = if unchecked || representation_changed {
            BTreeSet::new()
        } else {
            self.snapshot
                .body()
                .file_candidates(
                    &terms,
                    &self.filters.borrow(),
                    |path| policy.language_for(Path::new(path)),
                    &changed,
                    &mut self.work.borrow_mut(),
                )?
                .into_iter()
                .collect()
        };
        let mut evidence = BTreeMap::new();
        let mut match_lines: BTreeMap<NodeId, crate::evidence::MatchContext> = BTreeMap::new();
        let mut nodes = Vec::new();
        let mut retrieval = BTreeMap::new();
        let mut files_scanned = 0u64;
        for entry in report.entries {
            self.work.borrow().check()?;
            if !self.filters.borrow().matches(&entry.rel, entry.language) {
                continue;
            }
            let facts = self.snapshot.source_files().get(&entry.rel);
            let needs_scan = unchecked
                || representation_changed
                || changed.contains(&entry.rel)
                || facts.is_none_or(|file| file.truncated || file.version < 2);
            if !needs_scan && !candidates.contains(&entry.rel) {
                continue;
            }
            if sources
                .read_with_work(root, &entry.rel, &mut self.work.borrow_mut())?
                .is_none()
            {
                continue;
            }
            let (Some(text), Some(hash)) = (sources.text(&entry.rel), sources.hash(&entry.rel))
            else {
                continue;
            };
            files_scanned = files_scanned.saturating_add(1);
            let matches = self.work.borrow_mut().verify_positions(predicate, text)?;
            if matches.witnesses.is_empty() {
                continue;
            }
            let spans = witness_spans(text, &matches.witnesses, &self.work.borrow())?;
            let current_facts =
                facts.filter(|file| !representation_changed && file.source_hash == hash);
            // Source units locate declaration identities, never bound a phrase.
            // A declaration's complete span must enclose the verified witness.
            let mut owners = Vec::new();
            let mut documents = BTreeMap::new();
            let mut seen = BTreeSet::new();
            if let Some(file) = current_facts {
                for unit in &file.units {
                    if !self.work.borrow_mut().metadata_entry()? {
                        break;
                    }
                    if let Some(document) = &unit.documentation {
                        documents
                            .entry(document.span.start_byte)
                            .or_insert_with(|| document.clone());
                    }
                    if let Some(id) = &unit.owner
                        && seen.insert(id.clone())
                        && let Some(node) = self.read_node(id)?
                        && node.span.is_some()
                    {
                        owners.push(node);
                    }
                }
            }
            owners.sort_by(|a, b| {
                let width = |node: &Node| {
                    node.span
                        .map_or(u32::MAX, |s| s.end_byte.saturating_sub(s.start_byte))
                };
                width(a).cmp(&width(b)).then(a.id.cmp(&b.id))
            });
            for span in spans {
                let mut owner = None;
                for node in &owners {
                    if !self.work.borrow_mut().metadata_entry()? {
                        break;
                    }
                    if node.span.is_some_and(|s| {
                        s.start_byte <= span.start_byte && s.end_byte >= span.end_byte
                    }) {
                        owner = Some(node.clone());
                        break;
                    }
                }
                let owner_id = owner.as_ref().map(|node| node.id.clone());
                let documentation = documents
                    .range(..=span.start_byte)
                    .next_back()
                    .map(|(_, document)| document)
                    .filter(|document| span.end_byte <= document.span.end_byte)
                    .cloned();
                let associated = match documentation
                    .as_ref()
                    .and_then(|document| document.documented_symbol.as_ref())
                {
                    Some(id) => self.read_node(id)?,
                    None => None,
                };
                let node = associated.or(owner).unwrap_or_else(|| Node {
                    id: NodeId::file(&entry.rel),
                    kind: NodeKind::File,
                    path: entry.rel.clone(),
                    language: entry.language,
                    content_hash: Some(hash.to_owned()),
                    ..Node::default()
                });
                let id = node.id.clone();
                if !evidence.contains_key(&id) {
                    if !self.work.borrow_mut().candidate()? {
                        continue;
                    }
                    let rank = nodes.len();
                    nodes.push(Scored::new(node, rank_score(rank)));
                    evidence.insert(
                        id.clone(),
                        SourceEvidence {
                            span,
                            kind: if documentation.is_some() {
                                graph_search_types::source::SourceUnitKind::DocumentationComment
                            } else {
                                crate::units::kind(
                                    &entry.rel,
                                    entry
                                        .language
                                        .unwrap_or(graph_search_types::Language::Unknown),
                                )
                            },
                            owner: owner_id,
                            documentation,
                            match_line: span.start_line,
                            source_hash: hash.to_owned(),
                            package: current_facts.and_then(|facts| facts.package.clone()),
                            package_ref: None,
                            package_scope_incomplete: current_facts
                                .is_some_and(|facts| facts.package_scope_incomplete),
                            live: current_facts.is_none(),
                        },
                    );
                    if query.retrieval.explain {
                        retrieval.insert(
                            id.clone(),
                            RetrievalEvidence {
                                body_rank: Some(
                                    u32::try_from(rank).unwrap_or(u32::MAX).saturating_add(1),
                                ),
                                ..RetrievalEvidence::default()
                            },
                        );
                    }
                }
                // Endpoint anchors have no invented per-line term membership.
                // Whole-phrase proof is the hash-bound byte span above.
                match_lines
                    .entry(id)
                    .or_default()
                    .regions
                    .push(crate::evidence::MatchRegion {
                        span,
                        lines: BTreeMap::from([(span.start_line, 0), (span.end_line, 0)]),
                        headings: Vec::new(),
                        fence: None,
                        table: None,
                    });
            }
        }
        let candidates = nodes.len();
        truncations.extend(result_truncations(candidates, k));
        let nodes = select_diverse(nodes, k as usize, query.retrieval.per_file);
        Ok(Seeds {
            plan: query.retrieval.explain.then(|| RetrievalPlan {
                omitted_boilerplate: Vec::new(),
                query: query.query.clone(),
                options: query.retrieval.clone(),
                terms,
                routes: vec![if query.retrieval.mode == ExploreMode::Phrase {
                    RetrievalRoute::Phrase
                } else {
                    RetrievalRoute::Near
                }],
            }),
            retrieval,
            coverage,
            nodes,
            evidence,
            match_lines,
            truncations,
            candidates,
            files_scanned,
        })
    }
}

/// Resolve overlapping witnesses with a single monotone scan of source bytes.
/// The small endpoint map is bounded by the request witness allowance; no
/// per-source-byte line map or per-witness source rescan is needed.
fn witness_spans(
    text: &str,
    witnesses: &[crate::positional::Witness],
    work: &crate::work::WorkBudget,
) -> Result<Vec<Span>> {
    let mut endpoints = BTreeMap::new();
    for witness in witnesses {
        endpoints.insert(witness.start, 0u32);
        endpoints.insert(witness.end.saturating_sub(1), 0u32);
    }
    let mut from = 0usize;
    let mut line = 1u32;
    for (&offset, value) in &mut endpoints {
        for chunk in text.as_bytes()[from..offset].chunks(1024) {
            work.check()?;
            for &byte in chunk {
                line = line.saturating_add(u32::from(byte == b'\n'));
            }
        }
        *value = line;
        from = offset;
    }
    witnesses
        .iter()
        .map(|witness| {
            Ok(Span::new(
                endpoints[&witness.start],
                endpoints[&witness.end.saturating_sub(1)],
                u32::try_from(witness.start)
                    .map_err(|_| Error::InvalidQuery("source offset exceeds u32".into()))?,
                u32::try_from(witness.end)
                    .map_err(|_| Error::InvalidQuery("source offset exceeds u32".into()))?,
            ))
        })
        .collect()
}
