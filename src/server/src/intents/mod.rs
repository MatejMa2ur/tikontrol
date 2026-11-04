//! Safe apply intents per feature area. Each module exposes `pub fn compile(v, r) -> Option<Result<Plan, String>>`
//! and is registered in `apply::AREAS`. Follow apply.rs: validate every field, tag created objects with
//! `comment=tik:<kind>:<key>`, give every op an exact inverse, and mark high-risk changes.
