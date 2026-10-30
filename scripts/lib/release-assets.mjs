export function assetName(name, version) {
  const formats = [
    [".app.tar.gz", "mac", "app.tar.gz", ""],
    [".msi", "win", "msi", ""],
    ["-setup.exe", "win", "exe", "-setup"],
    [".dmg", "mac", "dmg", ""],
    [".AppImage", "linux", "AppImage", ""],
    [".deb", "linux", "deb", ""],
    [".rpm", "linux", "rpm", ""],
  ];
  const format = formats.find(([ending]) => name.endsWith(ending));
  if (!format) throw new Error(`unclassified release asset: ${name}`);
  const [, os, extension, suffix] = format;
  const architecture = /aarch64|arm64/.test(name)
    ? "arm64"
    : /x64|x86_64|amd64/.test(name)
      ? "x64"
      : null;
  if (!architecture) throw new Error(`no architecture in asset name: ${name}`);
  return `Emploidle-${version}-${os}-${architecture}${suffix}.${extension}`;
}
