// Keeps the app sized to the visible area. iOS Safari does not shrink 100dvh for the on-screen keyboard;
// it pans the page instead, which would push a bottom-pinned composer under the keyboard.
export function trackViewport() {
  const vv = window.visualViewport;
  if (!vv) return;
  const root = document.documentElement;
  const update = () => {
    // Leave pinch-zoom alone; only follow keyboard-driven changes.
    if (vv.scale > 1.01) {
      root.style.removeProperty('--app-height');
      root.style.removeProperty('--app-offset');
      return;
    }
    root.style.setProperty('--app-height', `${Math.round(vv.height)}px`);
    root.style.setProperty('--app-offset', `${Math.round(vv.offsetTop)}px`);
  };
  vv.addEventListener('resize', update);
  vv.addEventListener('scroll', update);
  update();
}
