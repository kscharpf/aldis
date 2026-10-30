### Added
- `test-vectors/cop1/`: one engine scenario per cell of CCSDS 232.1 Table 5-1 (FOP-1, 300 cells) and Table 6-1 (FARM-1, 33 cells), "Not applicable" cells asserting `InvalidState`, and scenarios for the normal run, retransmission, the sliding window, the timer, suspension, and the FARM-1 window (COP-001, COP-002, COP-005, TV-011).
- `test-vectors/models/cop1.py`: the independent model of FOP-1 and FARM-1 behind those scenarios, written blind from the Blue Books (TV-023).
