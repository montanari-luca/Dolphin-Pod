# Cloud models with OpenRouter

OpenRouter gives you one key for many cloud models, so it is the quickest way to try different reasoning models.

1. Create an account at https://openrouter.ai and generate an API key.
2. In Dolphin Pod, open **Settings → Providers**, add **OpenRouter** and paste the key. It is stored in your
   system keychain, never in the app database.
3. Choose a reasoning model for your agents. OpenRouter reports which parameters each model supports, and the
   reasoning-effort control adapts to match.

With OpenRouter, agents can also use server-side web search. Other providers fall back to a built-in key-less
search engine, so every agent has web access either way.
