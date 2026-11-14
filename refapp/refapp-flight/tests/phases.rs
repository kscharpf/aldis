//! The per-step phases of the flight node through the public API: running steps 1, 2 and 3 of
//! docs/reference-application.md §2.1 one at a time gives the outputs of `step` (§8.1).

#[cfg(test)]
mod phases {
    use aldis::core::ids::{Apid, SeqCount};
    use aldis::spp::{PacketFields, PacketType, SequenceFlags, SpacePacketWriter, SppConfig};
    use refapp_flight::FlightNode;
    use refapp_scenario::generated::{apid, time};
    use refapp_scenario::{Delivered, Instant, Node, Output, StepInput};

    /// One recorded output.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Out {
        Unit(Vec<u8>),
        Packet(u16, Vec<u8>),
        PacketIn(u16, Vec<u8>),
        Event(u16, Vec<u64>),
    }

    #[derive(Default)]
    struct Rec(Vec<Out>);

    impl Output for Rec {
        fn unit(&mut self, octets: &[u8]) {
            self.0.push(Out::Unit(octets.to_vec()));
        }
        fn packet_out(&mut self, apid: Apid, octets: &[u8]) {
            self.0.push(Out::Packet(apid.get(), octets.to_vec()));
        }
        fn packet_in(&mut self, apid: Apid, octets: &[u8]) {
            self.0.push(Out::PacketIn(apid.get(), octets.to_vec()));
        }
        fn app_event(&mut self, code: u16, args: &[u64]) {
            self.0.push(Out::Event(code, args.to_vec()));
        }
    }

    /// The 6-octet T-field of the scenario (§5) at `us` after T0.
    fn t_field(us: u64) -> [u8; 6] {
        let seconds = u32::try_from(us / 1_000_000)
            .unwrap()
            .wrapping_add(time::T0_COARSE_S);
        let fraction = u16::try_from((us % 1_000_000).wrapping_mul(65_536) / 1_000_000).unwrap();
        let [s3, s2, s1, s0] = seconds.to_be_bytes();
        let [f1, f0] = fraction.to_be_bytes();
        [s3, s2, s1, s0, f1, f0]
    }

    /// A command packet (§3.3.5); `at` is the time tag in µs after T0, or `None` for immediate.
    fn command(seq: u16, function: u8, at: Option<u64>, args: &[u8]) -> Vec<u8> {
        let (mode, field) = at.map_or((0, [0; 6]), |us| (1, t_field(us)));
        let mut data = vec![function, mode];
        data.extend_from_slice(&field);
        data.extend_from_slice(args);
        let mut buffer = [0_u8; 64];
        let fields = PacketFields {
            packet_type: PacketType::Telecommand,
            apid: Apid::new(apid::COMMANDS).unwrap(),
            sequence_flags: SequenceFlags::Unsegmented,
            sequence_count: SeqCount::new(seq).unwrap(),
        };
        let mut writer =
            SpacePacketWriter::begin(&mut buffer, &SppConfig::DEFAULT, fields, 0, data.len())
                .unwrap();
        writer.user_data_mut().copy_from_slice(&data);
        let total = writer.finish(data.len()).unwrap();
        buffer[..total].to_vec()
    }

    const fn input<'a>(now: u64, delivered: &'a [&'a [u8]]) -> StepInput<'a> {
        StepInput {
            now: Instant::from_micros(now),
            delivered: Delivered::new(delivered),
            ops: &[],
            tx_slot: false,
        }
    }

    // req: APP-010, APP-012, APP-016, APP-129
    #[test]
    fn the_phases_one_by_one_give_the_outputs_of_step() {
        // After the catch-up no housekeeping or payload slot is left, so steps 4 and 5 of §2.1 output
        // nothing and `step` equals steps 1 to 3 (the node is at packet level: no step 6).
        let start = 32_000_000_u64;
        let mut dump_args = 0x0001_0000_u32.to_be_bytes().to_vec();
        dump_args.extend_from_slice(&300_u16.to_be_bytes());
        let units = [
            command(1, 0x01, Some(start), &[]),
            command(2, 0x01, Some(start + 250_000), &[]),
            command(3, 0x02, None, &dump_args),
        ];
        let delivered: Vec<&[u8]> = units.iter().map(Vec::as_slice).collect();

        let mut whole = FlightNode::new();
        let mut parts = FlightNode::new();
        // Both nodes first run the late housekeeping and payload slots (a late step catches up);
        // that output is not compared.
        let catch_up = input(31_000_000, &[]);
        let _ = whole.step(&catch_up, &mut Rec::default());
        let _ = parts.step(&catch_up, &mut Rec::default());
        let (mut whole_out, mut parts_out) = (Rec::default(), Rec::default());

        // The instant of the delivery, then the wakes of the queue and the dump, with no delivery.
        let mut now = start;
        let mut first = true;
        let mut steps = 0;
        loop {
            let units: &[&[u8]] = if first { &delivered } else { &[] };
            first = false;
            let input = input(now, units);
            let deadline = whole.step(&input, &mut whole_out);

            parts.handle_delivered(input.now, input.delivered, &mut parts_out);
            parts.execute_due(input.now, &mut parts_out);
            parts.dump_segments(input.now, &mut parts_out);

            assert_eq!(parts_out.0, whole_out.0, "outputs at {now} us");
            assert_eq!(parts, whole, "state at {now} us");
            steps += 1;
            match deadline {
                Some(at) => now = at.as_micros(),
                None => break,
            }
        }
        // The instant of the delivery, then the three packets of the 300-octet dump, 125 ms apart.
        assert!(steps >= 3, "{steps} steps");
        assert!(
            whole_out
                .0
                .iter()
                .any(|o| matches!(o, Out::Packet(a, _) if *a == apid::DUMP)),
            "the dump ran"
        );
        assert!(whole_out.0.iter().any(|o| matches!(o, Out::Event(..))));
    }

    // req: APP-010, APP-012, APP-129
    #[test]
    fn the_phases_then_a_step_without_delivery_give_one_step() {
        // The use of the docs of `handle_delivered`: the phases of the delivery, then `step` with
        // no delivered unit, which runs steps 2 to 6 again at the same instant. At 1 s a
        // housekeeping slot is due, so steps 4 and 5 output.
        let now = 1_000_000_u64;
        let mut dump_args = 0x0001_0000_u32.to_be_bytes().to_vec();
        dump_args.extend_from_slice(&300_u16.to_be_bytes());
        let units = [
            command(1, 0x01, None, &[]),
            command(2, 0x04, None, &[0]),
            command(3, 0x02, None, &dump_args),
        ];
        let delivered: Vec<&[u8]> = units.iter().map(Vec::as_slice).collect();

        let mut whole = FlightNode::new();
        let mut parts = FlightNode::new();
        let (mut whole_out, mut parts_out) = (Rec::default(), Rec::default());

        let with_units = input(now, &delivered);
        let deadline = whole.step(&with_units, &mut whole_out);

        parts.handle_delivered(with_units.now, with_units.delivered, &mut parts_out);
        parts.execute_due(with_units.now, &mut parts_out);
        parts.dump_segments(with_units.now, &mut parts_out);
        let parts_deadline = parts.step(&input(now, &[]), &mut parts_out);

        assert_eq!(parts_out.0, whole_out.0);
        assert_eq!(parts, whole);
        assert_eq!(parts_deadline, deadline);
        assert!(
            whole_out
                .0
                .iter()
                .any(|o| matches!(o, Out::Packet(a, _) if *a == apid::HOUSEKEEPING)),
            "a housekeeping packet at the instant"
        );
        // The dump packet due at the instant is sent once, not again by the second call.
        let dump_packets = whole_out
            .0
            .iter()
            .filter(|o| matches!(o, Out::Packet(a, _) if *a == apid::DUMP))
            .count();
        assert_eq!(dump_packets, 1);
    }
}
