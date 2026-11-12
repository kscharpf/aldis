//! The loss script (APP-163, plan D5, §11.6): `drop` entries by (direction, index) as §6.5.

use refapp_harness::scenario::{Case, Direction, Fault};
use refapp_harness::trace::Dir;

use crate::Error;

/// The units to drop, by direction and index (counted from 0 in the order the driver sees them
/// in that direction).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LossScript {
    entries: Vec<(Dir, u64)>,
}

impl LossScript {
    /// A script that drops the units `entries`.
    #[must_use]
    pub const fn new(entries: Vec<(Dir, u64)>) -> Self {
        LossScript { entries }
    }

    /// The script of a case's fault script: its `drop` entries, in script order (§6.5).
    ///
    /// # Errors
    /// If the case has a fault other than `drop`: the driver loses units, it does not delay,
    /// duplicate, truncate, or flip them (§11.6).
    pub fn from_case(case: &Case) -> Result<Self, Error> {
        let mut entries = Vec::with_capacity(case.faults.len());
        for (i, f) in case.faults.iter().enumerate() {
            match f.fault {
                Fault::Drop {} => entries.push((
                    match f.dir {
                        Direction::Down => Dir::Down,
                        Direction::Up => Dir::Up,
                    },
                    f.index,
                )),
                Fault::Duplicate {}
                | Fault::Delay { .. }
                | Fault::Flip { .. }
                | Fault::Truncate { .. } => {
                    return Err(Error::Script(format!(
                        "case `{}`: faults[{i}] is not a drop; the loss script has drops only",
                        case.id
                    )));
                }
            }
        }
        Ok(LossScript { entries })
    }

    /// The script of a list of drops written `dir:index`, separated by commas, `dir` being `up` or
    /// `down` (`up:2,up:10,down:32`): the loss script of a run that is not a case of the scenario
    /// (§11.6).
    ///
    /// # Errors
    /// If an entry is not `up:<n>` or `down:<n>`.
    pub fn from_spec(spec: &str) -> Result<Self, Error> {
        let mut entries = Vec::new();
        for item in spec.split(',').filter(|i| !i.is_empty()) {
            let (dir, index) = item.split_once(':').ok_or_else(|| {
                Error::Script(format!("drop `{item}`: expected `up:<n>` or `down:<n>`"))
            })?;
            let dir = match dir {
                "up" => Dir::Up,
                "down" => Dir::Down,
                other => {
                    return Err(Error::Script(format!(
                        "drop `{item}`: direction `{other}` is not `up` or `down`"
                    )));
                }
            };
            let index = index
                .parse::<u64>()
                .map_err(|e| Error::Script(format!("drop `{item}`: {e}")))?;
            entries.push((dir, index));
        }
        Ok(LossScript { entries })
    }

    /// Whether the unit `index` in direction `dir` is dropped.
    #[must_use]
    pub fn drops(&self, dir: Dir, index: u64) -> bool {
        self.entries.contains(&(dir, index))
    }

    /// The entries, in script order.
    #[must_use]
    pub fn entries(&self) -> &[(Dir, u64)] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use refapp_harness::scenario::Scenario;
    use refapp_harness::trace::Dir;

    use super::LossScript;

    #[test]
    fn the_script_of_a_lossy_case_is_its_drops() {
        // req: APP-163
        let s = Scenario::load().unwrap();
        let script = LossScript::from_case(s.case("a3-uplink-loss").unwrap()).unwrap();
        assert_eq!(script.entries().len(), 9);
        assert!(script.drops(Dir::Up, 2));
        assert!(script.drops(Dir::Up, 10));
        assert!((33..40).all(|i| script.drops(Dir::Down, i)));
        assert!(!script.drops(Dir::Down, 2));
        assert!(!script.drops(Dir::Up, 33));
        let none = LossScript::from_case(s.case("a3-nominal").unwrap()).unwrap();
        assert_eq!(none.entries(), []);
    }

    #[test]
    fn a_script_is_read_from_a_list_of_drops() {
        // req: APP-163
        let script = LossScript::from_spec("up:3,up:7,down:25").unwrap();
        assert_eq!(
            script.entries(),
            [(Dir::Up, 3), (Dir::Up, 7), (Dir::Down, 25)]
        );
        assert_eq!(LossScript::from_spec("").unwrap().entries(), []);
        for bad in ["up", "left:1", "up:x", "down:-1"] {
            assert!(LossScript::from_spec(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_fault_other_than_drop_is_refused() {
        // req: APP-163
        let s = Scenario::load().unwrap();
        let err = LossScript::from_case(s.case("a2-uplink-faults").unwrap()).unwrap_err();
        assert!(err.to_string().contains("not a drop"), "{err}");
    }
}
