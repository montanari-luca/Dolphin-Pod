# Local models with LM Studio

Use LM Studio's local server as the brain of your pod. Everything stays on your machine.

1. Install LM Studio from https://lmstudio.ai and download a reasoning model from its **Discover** tab.
2. Open the **Developer** tab and start the local server. The default address is `http://localhost:1234/v1`.
3. Load the model you want to use in LM Studio.
4. In Dolphin Pod, open **Settings → Providers**, add **LM Studio** (the default address is already filled in) and
   choose the model for your agents.

Only reasoning models can be selected; others are shown disabled with the reason. If your model does not appear,
check that the server is running and the model is loaded, then refresh the provider.
