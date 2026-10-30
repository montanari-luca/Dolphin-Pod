// Single source of truth for site-wide constants and marketing data.

export const SITE = {
  name: "Dolphin Pod",
  tagline: "Your pod of AI agents, in one chat.",
  description:
    "Dolphin Pod is a Discord-like app where the members are AI agents: Roger moderates, Lincoln orchestrates and a team of specialists answer in shared chats. Local-first, private, on the model you choose.",
  repo: "https://github.com/montanari-luca/Dolphin-Pod",
  docsEditBase: "https://github.com/montanari-luca/Dolphin-Pod/edit/master/docs/",
};

export const CREDITS = {
  company: "MintFlow Technologies",
  companyUrl: "https://mintflow.dev",
  lead: "Luca Montanari",
  leadHandle: "@montanarii.luca",
  leadUrl: "https://instagram.com/montanarii.luca",
  year: 2026,
  license: "All rights reserved. Dolphin Pod is proprietary software.",
};

/** Same file as UPDATE_MANIFEST_URL in crates/dolphin-core/src/update.rs. */
export const MANIFEST_URL =
  "https://raw.githubusercontent.com/montanari-luca/MintFlow-Releases/refs/heads/main/dolphinpod.json";

export const PLATFORMS = [
  { id: "macos", label: "macOS", req: "macOS 11 Big Sur or newer" },
  {
    id: "windows",
    label: "Windows",
    req: "Windows 10 or newer, 64-bit (uses the Microsoft WebView2 runtime)",
  },
  { id: "linux", label: "Linux", req: "64-bit distribution with WebKitGTK 4.1 and a Secret Service keyring" },
  { id: "ios", label: "iOS", req: "iPhone (arrives together with encrypted sync and remote control)" },
] as const;
export type PlatformId = (typeof PLATFORMS)[number]["id"];

export const NAV = [
  { href: "/features/", label: "Features" },
  { href: "/screenshots/", label: "Screenshots" },
  { href: "/docs/getting-started/", label: "Docs", match: "/docs/" },
  { href: "/guides/", label: "Guides" },
  { href: "/download/", label: "Download" },
];

type Agent = { name: string; role: string; job: string; from: string; to: string };
export const AGENTS: Agent[] = [
  {
    name: "Roger",
    role: "Moderator",
    job: "Answers general questions, routes every request, checks on stuck agents.",
    from: "#3d8bfd",
    to: "#a77bdb",
  },
  {
    name: "Lincoln",
    role: "Orchestrator",
    job: "Works behind the scenes: briefs specialists and enforces permissions.",
    from: "#4f46e5",
    to: "#0ea5e9",
  },
  {
    name: "Rick",
    role: "Coding",
    job: "Writes, refactors and reviews code in any language.",
    from: "#2563eb",
    to: "#22d3ee",
  },
  {
    name: "Micheal",
    role: "Long horizon",
    job: "Plans and carries out big, multi-step projects end to end.",
    from: "#7c3aed",
    to: "#f59e0b",
  },
  {
    name: "Viktor",
    role: "Security",
    job: "Threat models, reviews code for vulnerabilities, hardens systems.",
    from: "#dc2626",
    to: "#7f1d1d",
  },
  {
    name: "Jordan",
    role: "Debugging",
    job: "Reads stack traces and logs, reproduces bugs, finds root causes.",
    from: "#f97316",
    to: "#facc15",
  },
  {
    name: "Emma",
    role: "Creative",
    job: "Stories, poems, scripts, copy and brainstorming.",
    from: "#ec4899",
    to: "#f472b6",
  },
  {
    name: "Ducky",
    role: "Docs",
    job: "READMEs, API references, guides and clear explanations of code.",
    from: "#eab308",
    to: "#fde047",
  },
  {
    name: "Sarah",
    role: "Images",
    job: "A reasoning model writes the prompt, an image model renders it.",
    from: "#f7826f",
    to: "#fbb36a",
  },
  {
    name: "Prof",
    role: "Teaching",
    job: "Teaches step by step, with examples and checks for understanding.",
    from: "#10b981",
    to: "#14b8a6",
  },
  {
    name: "Ruby",
    role: "Fun facts",
    job: "Tells surprising, true fun facts whenever you ask.",
    from: "#e11d48",
    to: "#fb7185",
  },
  {
    name: "Amelia",
    role: "Verification",
    job: "Double-checks other agents' work with evidence, only when you ask.",
    from: "#0d9488",
    to: "#2dd4bf",
  },
  {
    name: "Luna",
    role: "Research",
    job: "Parallel web searches, source and date checks, then a cited report.",
    from: "#4338ca",
    to: "#818cf8",
  },
  {
    name: "Robert",
    role: "Git",
    job: "Tracks history and manages commits, push, pull and fetch, usually for the coding agents.",
    from: "#ea580c",
    to: "#f59e0b",
  },
  {
    name: "DP Council",
    role: "Council",
    job: "The strongest model in the pod criticizes big tasks once before the work starts, then steps out.",
    from: "#facc15",
    to: "#a16207",
  },
  {
    name: "Vera",
    role: "Director",
    job: "Runs the development chats: orders the team, reviews the engineering plan before code is written.",
    from: "#0f766e",
    to: "#a3e635",
  },
  {
    name: "Nolan",
    role: "Engineering",
    job: "Architecture and systems: components, boundaries, data flow and risks, before the coder starts.",
    from: "#1d4ed8",
    to: "#38bdf8",
  },
  {
    name: "Priya",
    role: "Engineering",
    job: "Interfaces and quality: data model, edge cases and test strategy, before the coder starts.",
    from: "#9333ea",
    to: "#38bdf8",
  },
  {
    name: "Mira",
    role: "Design",
    job: "Idea A: the bold, expressive direction. Works in parallel with Kai.",
    from: "#db2777",
    to: "#fb923c",
  },
  {
    name: "Kai",
    role: "Design",
    job: "Idea B: the minimal, practical direction. You pick an idea or merge them.",
    from: "#0891b2",
    to: "#86efac",
  },
  {
    name: "Tilda",
    role: "Level design",
    job: "Games: layouts, pacing, encounters and the difficulty curve.",
    from: "#65a30d",
    to: "#fde047",
  },
  {
    name: "Jules",
    role: "3D art",
    job: "Games: models, materials, rigging, asset pipelines and budgets.",
    from: "#c026d3",
    to: "#6366f1",
  },
  {
    name: "Sonia",
    role: "Audio",
    job: "Games: music, sound effects, adaptive audio and mixing.",
    from: "#e11d48",
    to: "#7c3aed",
  },
  {
    name: "Wren",
    role: "Story",
    job: "Games: narrative, characters, dialogue, quests and lore.",
    from: "#b45309",
    to: "#fcd34d",
  },
];

export const PROVIDERS = {
  local: ["Ollama", "LM Studio", "OpenAI-compatible"],
  cloud: [
    "OpenRouter",
    "OpenAI",
    "Anthropic",
    "Google Gemini",
    "xAI",
    "Mistral",
    "DeepSeek",
    "Groq",
    "Together",
    "Fireworks",
    "Azure OpenAI",
  ],
};
