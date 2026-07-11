#[derive(Debug, Clone, PartialEq)]
pub struct IcpcEntry {
    pub id: String,
    pub solved_problems: u64,
    pub penalty_count: u64,
    pub time_using: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IcpcOptions {
    pub penalty_seconds: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedEntry {
    pub entry: IcpcEntry,
    pub rank: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IcpcRanker {
    penalty_seconds: u64,
}

impl IcpcRanker {
    pub fn new(options: IcpcOptions) -> Self {
        Self {
            penalty_seconds: options.penalty_seconds,
        }
    }

    pub fn penalty(&self, entry: &IcpcEntry) -> u64 {
        entry.time_using + entry.penalty_count * self.penalty_seconds
    }

    pub fn sort(&self, entries: &[IcpcEntry]) -> Vec<IcpcEntry> {
        let mut result = entries.to_vec();
        result.sort_unstable_by(|a, b| match b.solved_problems.cmp(&a.solved_problems) {
            std::cmp::Ordering::Equal => self.penalty(a).cmp(&self.penalty(b)),
            other => other,
        });
        result
    }

    pub fn rank(&self, entries: &[IcpcEntry]) -> Vec<RankedEntry> {
        let sorted = self.sort(entries);
        let mut result = Vec::with_capacity(sorted.len());
        let mut rank = 1u64;

        for (i, current) in sorted.into_iter().enumerate() {
            if i == 0 {
                result.push(RankedEntry { entry: current, rank });
                continue;
            }

            let previous = &result[i - 1].entry;
            let prev_penalty = self.penalty(previous);
            let curr_penalty = self.penalty(&current);

            let is_tie = current.solved_problems == previous.solved_problems && curr_penalty == prev_penalty;

            if is_tie {
                result.push(RankedEntry { entry: current, rank });
            } else {
                rank = (i + 1) as u64;
                result.push(RankedEntry { entry: current, rank });
            }
        }

        result
    }
}
