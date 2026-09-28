/**
 * Product constants owned by FubuMem rather than inherited from upstream Atlas.
 *
 * Kept in one module so the fork's outward-facing identity (where issues go,
 * which hosted services exist) is decided in exactly one place, and upstream
 * files only import names from here instead of carrying their own literals.
 */

/** The FubuMem repository: docs, issues, discussions and releases. */
export const FUBUMEM_REPO_URL = "https://github.com/FMsongX2/FubuMem";

/** Upstream Atlas, credited wherever the product describes its origin. */
export const UPSTREAM_ATLAS_URL = "https://github.com/pacifio/atlas";

export const FUBUMEM_DOCS_URL = `${FUBUMEM_REPO_URL}#readme`;
export const FUBUMEM_ISSUES_URL = `${FUBUMEM_REPO_URL}/issues`;
export const FUBUMEM_DISCUSSIONS_URL = `${FUBUMEM_REPO_URL}/discussions`;
export const FUBUMEM_RELEASES_URL = `${FUBUMEM_REPO_URL}/releases`;

/**
 * Upstream Atlas's hosted services (sign-in, organisations, team chat, sync,
 * the AI gateway and its credits) are operated by the Atlas team. FubuMem
 * does not talk to them, so every surface that needs them stays hidden while
 * this is false. The backend enforces the same rule independently.
 */
export const HOSTED_SERVICES_ENABLED = false;

/**
 * FubuMem builds carry no analytics key, so the upstream telemetry client is
 * inert and its settings would be switches that do nothing. They stay hidden.
 */
export const TELEMETRY_AVAILABLE = false;
