use sea_orm::{DeriveActiveEnum, EnumIter};

/// REVIEW: Should we use this translation?
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "u64", db_type = "Integer")]
pub enum Difficulty {
    /// 入门
    A = 0,
    /// 普及-
    B = 1,
    /// 普及/提高-
    C = 2,
    /// 普及+/提高
    D = 3,
    /// 提高+/省选-
    E = 4,
    /// 省选/NOI-
    F = 5,
    /// NOI/NOI+/CTSC
    G = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "u64", db_type = "Integer")]
pub enum CaseType {
    Hidden = 0,
    Example = 1,
}
