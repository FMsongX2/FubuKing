/**
 * Product constants owned by FubuKing rather than inherited from upstream Atlas.
 *
 * Kept in one module so the fork's outward-facing identity (where issues go,
 * which hosted services exist) is decided in exactly one place, and upstream
 * files only import names from here instead of carrying their own literals.
 */

/** The FubuKing repository: docs, issues, discussions and releases. */
export const FUBUKING_REPO_URL = "https://github.com/FMsongX2/FubuKing";

/** Upstream Atlas, credited wherever the product describes its origin. */
export const UPSTREAM_ATLAS_URL = "https://github.com/pacifio/atlas";

export const FUBUKING_DOCS_URL = `${FUBUKING_REPO_URL}#readme`;
export const FUBUKING_ISSUES_URL = `${FUBUKING_REPO_URL}/issues`;
export const FUBUKING_DISCUSSIONS_URL = `${FUBUKING_REPO_URL}/discussions`;
export const FUBUKING_RELEASES_URL = `${FUBUKING_REPO_URL}/releases`;

/**
 * Upstream Atlas's hosted services (sign-in, organisations, team chat, sync,
 * the AI gateway and its credits) are operated by the Atlas team. FubuKing
 * does not talk to them, so every surface that needs them stays hidden while
 * this is false. The backend enforces the same rule independently.
 */
export const HOSTED_SERVICES_ENABLED = false;

/**
 * FubuKing builds carry no analytics key, so the upstream telemetry client is
 * inert and its settings would be switches that do nothing. They stay hidden.
 */
export const TELEMETRY_AVAILABLE = false;
