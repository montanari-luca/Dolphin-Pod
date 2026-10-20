# Local models with Ollama

Run everything on your own machine: Ollama serves the model, Dolphin Pod talks to it, and nothing leaves your computer.

1. Install Ollama from https://ollama.com and start it.
2. Pull a thinking model, for example:

   ```bash
   ollama pull qwen3.5:2b
   ```

   Larger options: `qwen3.5:9b`, `gpt-oss:20b` (supports low / medium / high effort), `deepseek-r1:14b`.
3. In Dolphin Pod, add the Ollama provider (default `http://localhost:11434`) and choose the model for your agents.

Models are checked with Ollama's own capability list; only models that support thinking are selectable.
