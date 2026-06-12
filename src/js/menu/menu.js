// ============== 菜单卡片控制 ==============

import { getIsMenuOpen, setIsMenuOpen } from "../state/state.js";

export function toggleMenu() {
  if (getIsMenuOpen()) {
    closeMenu();
  } else {
    openMenu();
  }
}

export function openMenu() {
  const card = document.getElementById("menu-card");
  const btn = document.getElementById("btn-menu");
  card.classList.add("visible");
  card.setAttribute("aria-hidden", "false");
  btn.classList.add("active");
  setIsMenuOpen(true);
}

export function closeMenu() {
  const card = document.getElementById("menu-card");
  const btn = document.getElementById("btn-menu");
  card.classList.remove("visible");
  card.setAttribute("aria-hidden", "true");
  btn.classList.remove("active");
  setIsMenuOpen(false);
}

export function isMenuOpen() {
  return getIsMenuOpen();
}
