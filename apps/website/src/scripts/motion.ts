// Loads the Motion-based effects only on pages that actually use them, so docs pages ship no animation JS.
if (document.querySelector("[data-reveal], [data-stagger], [data-float]")) {
  void import("./effects");
}
