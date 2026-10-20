# Providers and models

Local: **Ollama**, **LM Studio**, any **OpenAI-compatible** server.
Cloud: **OpenRouter**, **OpenAI**, **Anthropic**, **Google Gemini**, **xAI**, **Mistral**, **DeepSeek**, **Groq**,
**Together**, **Fireworks**, **Azure OpenAI**.

Only reasoning models can be chosen. Dolphin Pod decides using the provider's own metadata where available
(OpenRouter supported parameters, Ollama capabilities, Anthropic and Gemini model capabilities) and otherwise a
bundled catalog (`packages/shared/catalog/models.json`). Non-reasoning models are shown disabled with the reason.

The reasoning-effort control shows only what the selected model supports: levels (for example low / medium / high),
a thinking-token budget, an on/off switch, or nothing.

Keys are stored in the system keychain and never written to the database.

Setup guides: [Ollama](guides/ollama.md), [LM Studio](guides/lm-studio.md), [OpenRouter](guides/openrouter.md).
