//! macOS specifics.
//!
//! **Security-scoped bookmarks: not needed for v1, stubbed.** They are required only for App Sandbox apps. SPYDER
//! Bone v1 ships unsigned / ad-hoc signed and NOT sandboxed (DECISIONS 44; PLAN section 2.3), so a folder chosen
//! once can be reopened by path after a restart. What macOS does enforce is TCC: the first access to Desktop,
//! Documents, Downloads, iCloud Drive, removable or network volumes shows a system prompt, and an ad-hoc signed
//! build may be asked again after every rebuild (its code identity changes). When access is denied, the folder
//! reads as missing and the watch shows "folder not available" until access is granted.
//!
//! TODO(macOS, sandboxed builds only): if the app is ever sandboxed (Mac App Store or a Developer ID build with
//! the sandbox entitlement), persist a security-scoped bookmark per watched folder
//! (`NSURL bookmarkDataWithOptions:NSURLBookmarkCreationWithSecurityScope`), resolve it on launch, call
//! `startAccessingSecurityScopedResource` before watching and `stop...` after, and prompt again when it is stale.
//! The settings file would carry the bookmark bytes (base64) next to the folder path.

/// Placeholder: returns the path unchanged (no sandbox, so no bookmark is needed).
pub fn resolve_folder_access(path: &std::path::Path) -> std::path::PathBuf {
    path.to_path_buf()
}
