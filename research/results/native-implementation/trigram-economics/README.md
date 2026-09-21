# Initial resident trigram diagnostic

This native prototype capture isolates resident gram filtering and `str::contains`
verification, with separate production direct-scan and read/hash timings. It does
not compare the same matcher on both deployment paths: resident matching is slower
than the production matcher for some patterns. Its resident break-even estimates
must not be interpreted as production adoption evidence. Build timing includes
index teardown. The captured probe source and input fingerprints are retained.

Use the [cumulative production-scanner experiment](../trigram-cumulative/README.md)
for the conditional adoption decision. No production trigram index was added.
