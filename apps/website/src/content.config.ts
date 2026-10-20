import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";

// The docs live in the repo's top-level docs/ folder and are rendered here as-is (no copies).
// docs/README.md is the repo index and is not published.
const docs = defineCollection({
  loader: glob({ pattern: ["**/*.md", "!README.md"], base: "../../docs" }),
});

export const collections = { docs };
