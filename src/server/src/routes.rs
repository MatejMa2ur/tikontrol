//! Feature routes beyond the core API (history, Safe apply, alerts). Each returns None when the
//! path isn't its own, so `api::route` falls through to 404.

use crate::app::App;
use crate::auth::Session;
use crate::http::{Req, Resp};
use std::sync::Arc;

pub fn extra(_app: &Arc<App>, _req: &Req, _s: &Session) -> Option<Resp> {
    None
}
