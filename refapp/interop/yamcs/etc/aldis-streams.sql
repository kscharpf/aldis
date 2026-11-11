-- Tables that record what the TM frame link emits, so that a run can be read back through the HTTP
-- API without a mission database (AINT WP-3). Written by this project.
-- The link creates these streams when they do not exist; they are created here, with the columns the
-- link gives them, because this file runs before the links start.
CREATE STREAM good_frames (rectime TIMESTAMP, seq INT, ertime HRES_TIMESTAMP, scid INT, vcid INT, frameSeq LONG, data BINARY)
CREATE STREAM bad_frames (rectime TIMESTAMP, seq INT, ertime HRES_TIMESTAMP, scid INT, vcid INT, frameSeq LONG, data BINARY)
CREATE STREAM clcw (clcw INT)
CREATE TABLE IF NOT EXISTS frames_good (rectime TIMESTAMP, seq INT, scid INT, vcid INT, frameSeq LONG, data BINARY, PRIMARY KEY (rectime, seq))
CREATE TABLE IF NOT EXISTS frames_bad (rectime TIMESTAMP, seq INT, scid INT, vcid INT, frameSeq LONG, data BINARY, PRIMARY KEY (rectime, seq))
CREATE TABLE IF NOT EXISTS clcw_log (n LONG AUTO_INCREMENT, clcw INT, PRIMARY KEY (n))
CREATE TABLE IF NOT EXISTS pkt_vc0 (gentime TIMESTAMP, seqNum INT, packet BINARY, PRIMARY KEY (gentime, seqNum))
CREATE TABLE IF NOT EXISTS pkt_vc1 (gentime TIMESTAMP, seqNum INT, packet BINARY, PRIMARY KEY (gentime, seqNum))
CREATE TABLE IF NOT EXISTS pkt_vc7 (gentime TIMESTAMP, seqNum INT, packet BINARY, PRIMARY KEY (gentime, seqNum))
INSERT INTO frames_good SELECT rectime, seq, scid, vcid, frameSeq, data FROM good_frames
INSERT INTO frames_bad SELECT rectime, seq, scid, vcid, frameSeq, data FROM bad_frames
INSERT INTO clcw_log SELECT clcw FROM clcw
INSERT INTO pkt_vc0 SELECT gentime, seqNum, packet FROM tm_vc0
INSERT INTO pkt_vc1 SELECT gentime, seqNum, packet FROM tm_vc1
INSERT INTO pkt_vc7 SELECT gentime, seqNum, packet FROM tm_vc7
