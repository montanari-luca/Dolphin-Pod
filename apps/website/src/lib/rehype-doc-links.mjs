import { posix } from "node:path";

// Docs are written for GitHub, so they link to each other as relative `*.md` files.
// Rewrite those to the website routes: docs/foo.md -> /docs/foo/, docs/guides/x.md -> /guides/x/.
export function rehypeDocLinks() {
  return (tree, file) => {
    const path = String(file.path ?? "").replaceAll("\\", "/");
    const idx = path.lastIndexOf("/docs/");
    if (idx === -1) return;
    const dir = posix.dirname(path.slice(idx + "/docs/".length));
    const visit = (node) => {
      if (node.type === "element" && node.tagName === "a" && typeof node.properties?.href === "string") {
        const m = node.properties.href.match(/^(?![a-z][a-z0-9+.-]*:|#|\/)([^?#]+)\.md(#.*)?$/i);
        if (m) {
          const target = posix.normalize(posix.join(dir, m[1]));
          const route = target.startsWith("guides/") ? `/${target}/` : `/docs/${target}/`;
          node.properties.href = route + (m[2] ?? "");
        }
      }
      node.children?.forEach(visit);
    };
    visit(tree);
  };
}
