//! Whether this build talks to upstream Atlas's hosted services.
//!
//! Sign-in, organisations, team chat, checkpoint sync and the AI gateway are
//! operated by the Atlas team, and every one of them is reached through a
//! signed-in account. FubuKing refuses to start that sign-in, so none of them
//! can be reached. The frontend mirrors this with `HOSTED_SERVICES_ENABLED`
//! in `src/lib/fubuking.ts` and hides the surfaces; this module is the
//! enforcement, so a hidden button that is somehow triggered still stops here.

/// FubuKing builds never connect to Atlas's hosted services.
pub const HOSTED_SERVICES_ENABLED: bool = false;

/// What a refused hosted-service command reports to the user.
pub const HOSTED_SERVICES_DISABLED: &str =
    "FubuKing does not connect to Atlas's hosted services (sign-in, organisations, sync).";

// A FubuKing build that turns hosted services on does not compile.
const _: () = assert!(!HOSTED_SERVICES_ENABLED);
