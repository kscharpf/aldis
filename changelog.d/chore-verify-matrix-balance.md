### Changed
- The hosted verification run's groups are balanced by measured durations (`fuzz-smoke` joins `coverage`, the Cortex-R exerciser joins `embedded`), and a part starts pulling the image while it checks out the repository; the runner's disk is cleared only when it has less than 40 GB free (VER-067).
