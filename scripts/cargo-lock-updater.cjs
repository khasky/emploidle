// commit-and-tag-version updater for src-tauri/Cargo.lock.
//
// Bumps the version of this crate's own [[package]] entry only, identified by
// its `name`. In Cargo.lock each package block lists `name` immediately
// followed by `version`, so we anchor on that adjacency and leave every
// dependency's pinned version untouched.
const NAME = "emploidle";
const ENTRY = new RegExp(`(name = "${NAME}"\\r?\\nversion = ")[^"]*(")`);

module.exports.readVersion = (contents) => {
  const match = new RegExp(`name = "${NAME}"\\r?\\nversion = "([^"]*)"`).exec(
    contents,
  );
  return match ? match[1] : undefined;
};

module.exports.writeVersion = (contents, version) =>
  contents.replace(ENTRY, `$1${version}$2`);
