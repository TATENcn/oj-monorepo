use algorithm::ranking::{IcpcEntry, IcpcOptions, IcpcRanker, RankedEntry};

fn opts(seconds: u64) -> IcpcOptions {
    IcpcOptions { penalty_seconds: seconds }
}

#[test]
fn penalty_returns_time_using_when_no_penalty_counts() {
    let ranker = IcpcRanker::new(opts(1200));
    let entry = IcpcEntry {
        id: "team-1".into(),
        solved_problems: 3,
        penalty_count: 0,
        time_using: 5400,
    };
    assert_eq!(ranker.penalty(&entry), 5400);
}

#[test]
fn penalty_adds_penalty_seconds_multiplied_by_penalty_counts() {
    let ranker = IcpcRanker::new(opts(1200));
    let entry = IcpcEntry {
        id: "team-1".into(),
        solved_problems: 2,
        penalty_count: 3,
        time_using: 3600,
    };
    // 3600 + 3 * 1200 = 7200
    assert_eq!(ranker.penalty(&entry), 7200);
}

#[test]
fn penalty_handles_zero_time_using_with_penalties() {
    let ranker = IcpcRanker::new(opts(1200));
    let entry = IcpcEntry {
        id: "team-new".into(),
        solved_problems: 0,
        penalty_count: 5,
        time_using: 0,
    };
    assert_eq!(ranker.penalty(&entry), 6000);
}

#[test]
fn penalty_handles_zero_penalty_seconds() {
    let ranker = IcpcRanker::new(opts(0));
    let entry = IcpcEntry {
        id: "team-1".into(),
        solved_problems: 1,
        penalty_count: 4,
        time_using: 100,
    };
    assert_eq!(ranker.penalty(&entry), 100);
}

fn teams() -> Vec<IcpcEntry> {
    vec![
        IcpcEntry {
            id: "A".into(),
            solved_problems: 3,
            penalty_count: 2,
            time_using: 3000,
        },
        IcpcEntry {
            id: "B".into(),
            solved_problems: 5,
            penalty_count: 1,
            time_using: 4000,
        },
        IcpcEntry {
            id: "C".into(),
            solved_problems: 3,
            penalty_count: 0,
            time_using: 4500,
        },
        IcpcEntry {
            id: "D".into(),
            solved_problems: 5,
            penalty_count: 3,
            time_using: 3500,
        },
        IcpcEntry {
            id: "E".into(),
            solved_problems: 1,
            penalty_count: 0,
            time_using: 1000,
        },
    ]
}

#[test]
fn sort_by_solved_problems_descending_first() {
    let ranker = IcpcRanker::new(opts(1200));
    let sorted = ranker.sort(&teams());
    let problems: Vec<u64> = sorted.iter().map(|t| t.solved_problems).collect();
    assert_eq!(problems, vec![5, 5, 3, 3, 1]);
}

#[test]
fn sort_by_penalty_ascending_within_same_solved_problems() {
    let ranker = IcpcRanker::new(opts(1200));
    let sorted = ranker.sort(&teams());
    let top_group: Vec<&str> = sorted.iter().filter(|t| t.solved_problems == 5).map(|t| t.id.as_str()).collect();
    assert_eq!(top_group, vec!["B", "D"]);
}

#[test]
fn sort_breaks_ties_by_penalty_within_same_problem_count() {
    let ranker = IcpcRanker::new(opts(1200));
    let sorted = ranker.sort(&teams());
    let mid_group: Vec<&str> = sorted.iter().filter(|t| t.solved_problems == 3).map(|t| t.id.as_str()).collect();
    assert_eq!(mid_group, vec!["C", "A"]);
}

#[test]
fn sort_places_team_with_fewest_solved_at_end() {
    let ranker = IcpcRanker::new(opts(1200));
    let sorted = ranker.sort(&teams());
    assert_eq!(sorted.last().unwrap().id, "E");
}

#[test]
fn sort_does_not_mutate_original_slice() {
    let ranker = IcpcRanker::new(opts(1200));
    let original = teams();
    let _sorted = ranker.sort(&original);
    assert_eq!(original, teams());
}

#[test]
fn sort_returns_new_instance() {
    let ranker = IcpcRanker::new(opts(1200));
    let original = teams();
    let sorted = ranker.sort(&original);
    let orig_ptr = original.as_ptr();
    let sorted_ptr = sorted.as_ptr();
    assert_ne!(orig_ptr, sorted_ptr);
}

#[test]
fn sort_handles_empty_slice() {
    let ranker = IcpcRanker::new(opts(1200));
    let sorted = ranker.sort(&[]);
    assert!(sorted.is_empty());
}

#[test]
fn sort_handles_single_entry() {
    let ranker = IcpcRanker::new(opts(1200));
    let entry = IcpcEntry {
        id: "solo".into(),
        solved_problems: 2,
        penalty_count: 1,
        time_using: 600,
    };
    assert_eq!(ranker.sort(&[entry.clone()]), vec![entry]);
}

#[test]
fn rank_assigns_sequential_ranks_when_no_ties_exist() {
    let ranker = IcpcRanker::new(opts(1200));
    let entries = vec![
        IcpcEntry {
            id: "A".into(),
            solved_problems: 3,
            penalty_count: 0,
            time_using: 1000,
        },
        IcpcEntry {
            id: "B".into(),
            solved_problems: 2,
            penalty_count: 0,
            time_using: 1000,
        },
        IcpcEntry {
            id: "C".into(),
            solved_problems: 1,
            penalty_count: 0,
            time_using: 1000,
        },
    ];
    let ranked = ranker.rank(&entries);
    let ranks: Vec<u64> = ranked.iter().map(|r| r.rank).collect();
    assert_eq!(ranks, vec![1, 2, 3]);
    let ids: Vec<&str> = ranked.iter().map(|r| r.entry.id.as_str()).collect();
    assert_eq!(ids, vec!["A", "B", "C"]);
}

#[test]
fn rank_assigns_tied_ranks_as_1_2_2_4_style() {
    let ranker = IcpcRanker::new(opts(1200));
    let entries = vec![
        IcpcEntry {
            id: "A".into(),
            solved_problems: 5,
            penalty_count: 0,
            time_using: 2000,
        },
        IcpcEntry {
            id: "B".into(),
            solved_problems: 5,
            penalty_count: 0,
            time_using: 2000,
        },
        IcpcEntry {
            id: "C".into(),
            solved_problems: 5,
            penalty_count: 1,
            time_using: 1000,
        },
        IcpcEntry {
            id: "D".into(),
            solved_problems: 3,
            penalty_count: 0,
            time_using: 500,
        },
    ];
    let ranked = ranker.rank(&entries);
    assert_eq!(ranked[0].rank, 1);
    assert_eq!(ranked[0].entry.id, "A");
    assert_eq!(ranked[1].rank, 1);
    assert_eq!(ranked[1].entry.id, "B");
    assert_eq!(ranked[2].rank, 3);
    assert_eq!(ranked[2].entry.id, "C");
    assert_eq!(ranked[3].rank, 4);
    assert_eq!(ranked[3].entry.id, "D");
}

#[test]
fn rank_same_rank_for_teams_tied_on_both_solved_and_penalty() {
    let ranker = IcpcRanker::new(opts(1200));
    let entries = vec![
        IcpcEntry {
            id: "X".into(),
            solved_problems: 4,
            penalty_count: 2,
            time_using: 6000,
        },
        IcpcEntry {
            id: "Y".into(),
            solved_problems: 4,
            penalty_count: 2,
            time_using: 6000,
        },
    ];
    let ranked = ranker.rank(&entries);
    assert_eq!(ranked[0].rank, 1);
    assert_eq!(ranked[1].rank, 1);
}

#[test]
fn rank_does_not_treat_different_penalties_as_tie() {
    let ranker = IcpcRanker::new(opts(1200));
    let entries = vec![
        IcpcEntry {
            id: "Fast".into(),
            solved_problems: 3,
            penalty_count: 0,
            time_using: 1000,
        },
        IcpcEntry {
            id: "Slow".into(),
            solved_problems: 3,
            penalty_count: 0,
            time_using: 2000,
        },
    ];
    let ranked = ranker.rank(&entries);
    assert_eq!(ranked[0].rank, 1);
    assert_eq!(ranked[1].rank, 2);
}

#[test]
fn rank_assigns_rank_1_to_all_when_all_entries_are_tied() {
    let ranker = IcpcRanker::new(opts(1200));
    let entries = vec![
        IcpcEntry {
            id: "A".into(),
            solved_problems: 1,
            penalty_count: 0,
            time_using: 100,
        },
        IcpcEntry {
            id: "B".into(),
            solved_problems: 1,
            penalty_count: 0,
            time_using: 100,
        },
        IcpcEntry {
            id: "C".into(),
            solved_problems: 1,
            penalty_count: 0,
            time_using: 100,
        },
    ];
    let ranked = ranker.rank(&entries);
    assert!(ranked.iter().all(|r| r.rank == 1));
}

#[test]
fn rank_preserves_sort_order_in_returned_entries() {
    let ranker = IcpcRanker::new(opts(1200));
    let entries = vec![
        IcpcEntry {
            id: "Low".into(),
            solved_problems: 1,
            penalty_count: 0,
            time_using: 100,
        },
        IcpcEntry {
            id: "Mid".into(),
            solved_problems: 2,
            penalty_count: 0,
            time_using: 200,
        },
        IcpcEntry {
            id: "High".into(),
            solved_problems: 3,
            penalty_count: 0,
            time_using: 300,
        },
    ];
    let ranked = ranker.rank(&entries);
    let ids: Vec<&str> = ranked.iter().map(|r| r.entry.id.as_str()).collect();
    assert_eq!(ids, vec!["High", "Mid", "Low"]);
}

#[test]
fn rank_handles_empty_entries() {
    let ranker = IcpcRanker::new(opts(1200));
    assert_eq!(ranker.rank(&[]), vec![]);
}

#[test]
fn rank_handles_single_entry() {
    let ranker = IcpcRanker::new(opts(1200));
    let ranked = ranker.rank(&[IcpcEntry {
        id: "One".into(),
        solved_problems: 1,
        penalty_count: 0,
        time_using: 0,
    }]);
    assert_eq!(
        ranked,
        vec![RankedEntry {
            entry: IcpcEntry {
                id: "One".into(),
                solved_problems: 1,
                penalty_count: 0,
                time_using: 0,
            },
            rank: 1,
        }]
    );
}

#[test]
fn rank_multi_way_tie_then_gap_then_another_tie() {
    let ranker = IcpcRanker::new(opts(1200));
    let entries = vec![
        IcpcEntry {
            id: "A".into(),
            solved_problems: 5,
            penalty_count: 0,
            time_using: 5000,
        },
        IcpcEntry {
            id: "B".into(),
            solved_problems: 5,
            penalty_count: 0,
            time_using: 5000,
        },
        IcpcEntry {
            id: "C".into(),
            solved_problems: 5,
            penalty_count: 0,
            time_using: 5000,
        },
        IcpcEntry {
            id: "D".into(),
            solved_problems: 4,
            penalty_count: 0,
            time_using: 4000,
        },
        IcpcEntry {
            id: "E".into(),
            solved_problems: 4,
            penalty_count: 0,
            time_using: 4000,
        },
    ];
    let ranked = ranker.rank(&entries);
    let ranks: Vec<u64> = ranked.iter().map(|r| r.rank).collect();
    assert_eq!(ranks, vec![1, 1, 1, 4, 4]);
}
