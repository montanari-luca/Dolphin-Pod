// Progressive-enhancement motion with the framework-agnostic Motion API.
// Everything is gated on prefers-reduced-motion, and content is visible by default
// (the hidden start state in global.css only exists for users who allow motion).
import { animate, inView, stagger } from "motion";

const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

if (!reduced) {
  // Section reveals: fade and rise once, when the element scrolls into view.
  document.querySelectorAll<HTMLElement>("[data-reveal]").forEach((el) => {
    const delay = Number(el.dataset.revealDelay ?? 0);
    inView(
      el,
      () => {
        animate(el, { opacity: [0, 1], transform: ["translateY(18px)", "translateY(0px)"] }, { duration: 0.7, delay, ease: [0.22, 1, 0.36, 1] });
      },
      { amount: 0.15 },
    );
  });

  // Staggered groups (agent roster, feature grids): children arrive one after another.
  document.querySelectorAll<HTMLElement>("[data-stagger]").forEach((group) => {
    const items = Array.from(group.querySelectorAll<HTMLElement>("[data-stagger-item]"));
    items.forEach((i) => {
      i.style.opacity = "0";
    });
    inView(
      group,
      () => {
        animate(
          items,
          { opacity: [0, 1], transform: ["translateY(16px) scale(0.97)", "translateY(0px) scale(1)"] },
          { duration: 0.55, delay: stagger(0.06), ease: [0.22, 1, 0.36, 1] },
        );
      },
      { amount: 0.2 },
    );
  });

  // Floating glass panels in the hero.
  document.querySelectorAll<HTMLElement>("[data-float]").forEach((el, i) => {
    const dist = Number(el.dataset.float) || 10;
    animate(
      el,
      { transform: ["translateY(0px)", `translateY(${-dist}px)`] },
      { duration: 4 + (i % 3), repeat: Infinity, repeatType: "reverse", ease: "easeInOut", delay: i * 0.4 },
    );
  });
}
