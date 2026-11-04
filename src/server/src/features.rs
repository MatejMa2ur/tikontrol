//! Routes for Safe apply, alerts and other write features (filled in by later features).

use crate::app::App;
use crate::auth::Session;
use crate::http::{Req, Resp};
use std::sync::Arc;

pub fn route(_app: &Arc<App>, _req: &Req, _s: &Session) -> Option<Resp> {
    None
}
