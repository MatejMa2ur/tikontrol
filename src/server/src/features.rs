//! Routes for Safe apply and other write features (filled in by later features). Alerts live in alerts.rs.

use crate::app::App;
use crate::auth::Session;
use crate::http::{Req, Resp};
use std::sync::Arc;

pub fn route(_app: &Arc<App>, _req: &Req, _s: &Session) -> Option<Resp> {
    None
}
