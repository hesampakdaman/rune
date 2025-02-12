use super::data::ArbitraryType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Status {
    Fail(String, Vec<Option<ArbitraryType>>),
    Abort(String),
    Pass,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Output {
    pub(crate) function: String,
    pub(crate) status: Status,
}
