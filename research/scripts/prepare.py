"""Write fixed discovery queries and synthetic semantic fixtures (no repo mutation)."""
import json,pathlib
base=pathlib.Path(__file__).resolve().parents[1]
sets={
'nanus':[
('render_content','crates/nanus-bundle/src/agent_loop.rs'),('render content','crates/nanus-bundle/src/agent_loop.rs'),('How does the agent run tools in parallel?','crates/nanus-bundle/src/agent_loop.rs'),('approval_reason','crates/nanus-bundle/src/agent_loop.rs'),('approval reason sandbox destructive','crates/nanus-bundle/src/agent_loop.rs'),('How is the step budget added to the prompt?','crates/nanus-bundle/src/agent_loop.rs'),('AgentRunner::run_turn','crates/nanus-bundle/src/agent_loop.rs'),('run_turn()','crates/nanus-bundle/src/agent_loop.rs'),('last assistant text','crates/nanus-bundle/src/agent_loop.rs'),('with_step_budget','crates/nanus-bundle/src/agent_loop.rs')],
'blogwright':[
('listPublishablePosts','packages/pds/src/content.ts'),('list publishable posts','packages/pds/src/content.ts'),('How are publishable posts selected?','packages/pds/src/content.ts'),('syncDocuments','packages/pds/src/sync.ts'),('sync documents','packages/pds/src/sync.ts'),('resolvePdsSecretName','packages/pds/src/config.ts'),('resolve pds secret name','packages/pds/src/config.ts'),('loadPdsSecret','packages/pds/src/secret.ts'),('load pds secret','packages/pds/src/secret.ts'),('publicationRecord()','packages/pds/src/sync.ts')],
'whatsurvey':[
('verifyWebhookSignature','workspaces/backend/src/core/whatsapp/signature.ts'),('verify webhook signature','workspaces/backend/src/core/whatsapp/signature.ts'),('How is the WhatsApp webhook signature verified?','workspaces/backend/src/core/whatsapp/signature.ts'),('encryptSecret','workspaces/backend/src/core/settings/crypto.ts'),('encrypt secret','workspaces/backend/src/core/settings/crypto.ts'),('getSurveyDraft','workspaces/backend/src/core/db/survey-versions.ts'),('get survey draft','workspaces/backend/src/core/db/survey-versions.ts'),('saveSurveyDraft','workspaces/backend/src/core/db/survey-versions.ts'),('publishSurveyDraft()','workspaces/backend/src/core/db/survey-versions.ts'),('copyWebhook','workspaces/frontend/src/lib/components/WhatsAppConfigurations.svelte')]
}
for repo,rows in sets.items():
 qs=[{'mode':'explore','query':q,'expected_path':p,'category':'exact' if ' ' not in q and '(' not in q else 'natural_or_split'} for q,p in rows]
 for q,p in rows:
  if ' ' not in q and '(' not in q:
   qs.extend([{'mode':mode,'query':q} for mode in ['symbol','callers','callees','refs','impact']])
 (base/'results'/f'{repo}-queries.json').write_text(json.dumps(qs,indent=2)+'\n')
fixture=base/'fixtures'/'semantics';fixture.mkdir(parents=True,exist_ok=True)
files={
'a.rs':'''struct A; struct B;
impl A { fn run(&self) { self.finish(); } fn finish(&self) {} }
impl B { fn run(&self) { self.finish(); } fn finish(&self) {} }
fn leaf() {} fn middle() { leaf(); } fn entry() { middle(); }
fn ambiguous() { mystery.finish(); }
fn nested() { factory().finish(); } fn factory() -> A { A }
fn take_callback() { consume(leaf); }
fn consume(f: fn()) { f(); }
fn shadow(leaf: fn()) { leaf(); }
''',
'z.rs':'fn leaf() {}\nfn foreign() { external::leaf(); }\n',
'a.ts':'''export function shared() {}\nexport function unique() {}\n''',
'b.ts':'''export function shared() {}\n''',
'consumer.ts':'''import {shared as renamed, unique} from './a';
function imported() { renamed(); unique(); }
function arbitrary() { external.unique(); }
function chained() { factory().unique(); }
function factory() { return {}; }
''',
'ui.svelte':'<script lang="ts">function clickButton() { console.log("clicked"); }</script><button onclick={clickButton}>Go</button>',
'notes.md':'Distinctivebodytoken appears only in prose.\n',
'web.html':'<link rel="stylesheet" href="web.css"><div class="card active" id="hero"></div>',
'web.css':'.card.active { color: red; } #hero { color: blue; }'
}
for p,t in files.items():(fixture/p).write_text(t)
qs=[{'mode':m,'query':q} for m,q in [('symbol','leaf'),('callees','A::run'),('callees','B::run'),('callees','ambiguous'),('callees','foreign'),('callees','nested'),('callees','shadow'),('callees','imported'),('refs','leaf'),('deps','consumer.ts'),('symbol','clickButton')]]
qs += [
{'mode':'symbol','query':'leaf','path':'z.rs','limit':1},
{'mode':'callers','query':'middle','path':'z.rs'},
{'mode':'explore','query':'Distinctivebodytoken','path':'*.rs','lang':'rust'},
{'mode':'explore','query':'entry leaf','hops':2},
{'mode':'explore','query':'leaf()'},
{'mode':'path','query':'middle','to':'middle'},
{'mode':'callers','query':'leaf','limit':1},
{'mode':'impact','query':'leaf','limit':1,'depth':4},
{'mode':'explore','query':'leaf','max_bytes':100},
]
(base/'results'/'semantics-queries.json').write_text(json.dumps(qs,indent=2)+'\n')
