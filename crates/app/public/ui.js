/* Themed dropdowns for the app's native <select> controls.
 *
 * The native select stays in the DOM as the value holder, so every existing
 * Rust `change` listener keeps working. We only replace its closed control and
 * popup with a themed combobox/listbox. Without JavaScript the native controls
 * remain untouched.
 */
(() => {
  "use strict";

  const MENU_ID = "cselect-menu";
  const MOBILE_QUERY = "(max-width: 560px)";
  let layer = null;
  let menu = null;
  let optionsBox = null;
  let sheetTitle = null;
  let activeSelect = null;
  let activeOptions = [];
  let activeIndex = -1;
  let typeahead = "";
  let typeaheadTimer = 0;

  const isElement = (value) => value instanceof Element;

  function selectedOption(select) {
    return select.options[select.selectedIndex] || select.options[0] || null;
  }

  function syncSelect(select) {
    const trigger = select._cselectTrigger;
    if (!trigger) return;
    const option = selectedOption(select);
    const value = trigger.querySelector(".cselect-value");
    if (value) {
      value.textContent = option ? option.textContent.trim() : "";
    }
    trigger.disabled = select.disabled;
    trigger.title = option ? option.textContent.trim() : "";
    if (activeSelect === select && activeOptions.length) {
      refreshSelection();
    }
  }

  function syncAll() {
    document.querySelectorAll("select[data-cselect]").forEach(syncSelect);
  }

  function labelFor(select) {
    const explicit = select.getAttribute("aria-label");
    if (explicit) return explicit;
    const field = select.closest(".filter-field");
    const span = field && field.querySelector(":scope > span");
    if (span && span.textContent.trim()) return span.textContent.trim();
    return select.id || "Select";
  }

  function refreshSelection() {
    if (!activeSelect) return;
    const current = activeSelect.value;
    activeOptions.forEach(({ button, option }) => {
      const selected =
        option.selected || (option.value === current && !option.disabled);
      button.classList.toggle("is-selected", selected);
      button.setAttribute("aria-selected", selected ? "true" : "false");
    });
  }

  function ensureMenu() {
    if (layer) return;
    layer = document.createElement("div");
    layer.className = "cselect-layer";
    layer.hidden = true;

    const backdrop = document.createElement("div");
    backdrop.className = "cselect-backdrop";
    backdrop.addEventListener("click", close);

    menu = document.createElement("div");
    menu.className = "cselect-menu";
    menu.id = MENU_ID;
    menu.setAttribute("role", "listbox");
    menu.tabIndex = -1;

    const head = document.createElement("div");
    head.className = "cselect-sheet-head";
    sheetTitle = document.createElement("span");
    sheetTitle.className = "cselect-sheet-title";
    const closeButton = document.createElement("button");
    closeButton.type = "button";
    closeButton.className = "cselect-sheet-close";
    closeButton.setAttribute("aria-label", "Cerrar");
    closeButton.textContent = "×";
    closeButton.addEventListener("click", close);
    head.append(sheetTitle, closeButton);

    optionsBox = document.createElement("div");
    optionsBox.className = "cselect-options";

    menu.append(head, optionsBox);
    layer.append(backdrop, menu);
    document.body.append(layer);

    menu.addEventListener("click", (event) => {
      const target = isElement(event.target) ? event.target : null;
      const button = target && target.closest(".cselect-option");
      if (!button || button.disabled) return;
      choose(Number(button.dataset.index));
    });
    menu.addEventListener("pointermove", (event) => {
      if (event.pointerType !== "mouse") return;
      const target = isElement(event.target) ? event.target : null;
      const button = target && target.closest(".cselect-option");
      if (!button || button.disabled) return;
      setActive(Number(button.dataset.index), false);
    });
  }

  function appendOption(option) {
    const index = activeOptions.length;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "cselect-option";
    button.id = `cselect-option-${index}`;
    button.dataset.index = String(index);
    button.dataset.value = option.value;
    button.setAttribute("role", "option");
    button.setAttribute("aria-selected", "false");
    button.disabled = option.disabled;

    const label = document.createElement("span");
    label.className = "cselect-option-label";
    label.textContent = option.textContent.trim();
    const check = document.createElement("span");
    check.className = "cselect-check";
    check.setAttribute("aria-hidden", "true");

    button.append(label, check);
    optionsBox.append(button);
    activeOptions.push({ button, option });
  }

  function renderOptions(select) {
    optionsBox.replaceChildren();
    activeOptions = [];
    for (const child of select.children) {
      if (child.tagName === "OPTGROUP") {
        const group = document.createElement("div");
        group.className = "cselect-group";
        group.setAttribute("role", "presentation");
        group.textContent = child.label;
        optionsBox.append(group);
        for (const option of child.children) {
          if (option.tagName === "OPTION") appendOption(option);
        }
      } else if (child.tagName === "OPTION") {
        appendOption(child);
      }
    }
  }

  function positionMenu() {
    if (!activeSelect) return;
    const trigger = activeSelect._cselectTrigger;
    const mobile = window.matchMedia(MOBILE_QUERY).matches;
    menu.classList.toggle("is-sheet", mobile);
    layer.classList.toggle("is-sheet", mobile);
    menu.classList.remove("is-above");
    menu.style.visibility = "hidden";
    menu.style.removeProperty("left");
    menu.style.removeProperty("top");
    menu.style.removeProperty("width");
    menu.style.removeProperty("max-height");

    if (mobile) {
      menu.style.visibility = "";
      return;
    }

    const rect = trigger.getBoundingClientRect();
    const margin = 8;
    const gap = 6;
    const minWidth =
      activeSelect.id === "language" ? 118 : activeSelect.id === "book-jump" ? 232 : 166;
    const width = Math.max(rect.width, minWidth);
    menu.style.width = `${width}px`;
    const maxHeight = Math.max(160, Math.min(340, window.innerHeight - margin * 2));
    menu.style.maxHeight = `${maxHeight}px`;
    const height = Math.min(menu.scrollHeight, maxHeight);
    const below = window.innerHeight - rect.bottom - gap - margin;
    const above = rect.top - gap - margin;

    let top;
    let maxH;
    if (below >= Math.min(height, 220) || below >= above) {
      top = rect.bottom + gap;
      maxH = Math.max(150, Math.min(maxHeight, below));
    } else {
      maxH = Math.max(150, Math.min(maxHeight, above));
      top = Math.max(margin, rect.top - gap - Math.min(height, maxH));
    }

    let left = rect.left;
    if (left + width > window.innerWidth - margin) {
      left = rect.right - width;
    }
    left = Math.max(margin, Math.min(left, window.innerWidth - margin - width));
    top = Math.max(margin, Math.min(top, window.innerHeight - margin - maxH));

    menu.style.left = `${left}px`;
    menu.style.top = `${top}px`;
    menu.style.maxHeight = `${maxH}px`;
    if (top + maxH < rect.top - gap) {
      menu.classList.add("is-above");
    }
    menu.style.visibility = "";
  }

  function setActive(index, scroll = true) {
    if (!activeOptions.length) return;
    const clamped = Math.max(0, Math.min(index, activeOptions.length - 1));
    activeIndex = clamped;
    activeOptions.forEach(({ button }, i) => {
      button.classList.toggle("is-active", i === clamped);
    });
    const entry = activeOptions[clamped];
    if (activeSelect && activeSelect._cselectTrigger) {
      activeSelect._cselectTrigger.setAttribute(
        "aria-activedescendant",
        entry ? entry.button.id : "",
      );
    }
    if (scroll && entry) {
      entry.button.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }

  function open(select) {
    ensureMenu();
    if (activeSelect) close(false);
    activeSelect = select;
    const trigger = select._cselectTrigger;
    renderOptions(select);
    refreshSelection();
    sheetTitle.textContent = labelFor(select);
    const closeButton = menu.querySelector(".cselect-sheet-close");
    if (closeButton) {
      closeButton.setAttribute(
        "aria-label",
        document.documentElement.lang === "en" ? "Close" : "Cerrar",
      );
    }
    layer.hidden = false;
    positionMenu();
    const selected = activeOptions.findIndex(({ option }) => option.selected);
    setActive(selected >= 0 ? selected : 0, true);
    trigger.setAttribute("aria-expanded", "true");
    trigger.setAttribute("aria-controls", MENU_ID);
    document.documentElement.classList.add("cselect-open");
  }

  function close(restoreFocus = false) {
    if (!activeSelect || !layer) return;
    const trigger = activeSelect._cselectTrigger;
    activeSelect = null;
    activeOptions = [];
    activeIndex = -1;
    typeahead = "";
    window.clearTimeout(typeaheadTimer);
    layer.hidden = true;
    layer.classList.remove("is-sheet");
    menu.classList.remove("is-sheet", "is-above");
    menu.style.visibility = "";
    document.documentElement.classList.remove("cselect-open");
    if (trigger) {
      trigger.setAttribute("aria-expanded", "false");
      trigger.removeAttribute("aria-activedescendant");
      if (restoreFocus) trigger.focus();
    }
  }

  function choose(index) {
    const entry = activeOptions[index];
    if (!entry || !activeSelect) return;
    const select = activeSelect;
    if (select.value !== entry.option.value) {
      select.value = entry.option.value;
      syncSelect(select);
      select.dispatchEvent(new Event("change", { bubbles: true }));
    }
    close();
    const trigger = select._cselectTrigger;
    if (trigger) trigger.focus();
  }

  function move(delta) {
    if (!activeOptions.length) return;
    setActive((activeIndex + delta + activeOptions.length) % activeOptions.length, true);
  }

  function typeaheadMatch(key) {
    if (key.length !== 1 || !/\S/.test(key)) return;
    typeahead += key.toLocaleLowerCase();
    window.clearTimeout(typeaheadTimer);
    typeaheadTimer = window.setTimeout(() => {
      typeahead = "";
    }, 700);
    const needle = typeahead;
    const ordered = activeOptions
      .map((entry, index) => ({ entry, index }))
      .filter(({ entry }) => !entry.option.disabled);
    const start = ordered.findIndex(({ index }) => index === activeIndex);
    const rotated =
      start >= 0 ? ordered.slice(start + 1).concat(ordered.slice(0, start + 1)) : ordered;
    const match = rotated.find(({ entry }) =>
      entry.button.textContent.trim().toLocaleLowerCase().startsWith(needle),
    );
    if (match) setActive(match.index, true);
  }

  function onTriggerKeydown(event, select) {
    const openNow = activeSelect === select;
    if (!openNow) {
      if (["ArrowDown", "ArrowUp", "Enter", " ", "Spacebar"].includes(event.key)) {
        event.preventDefault();
        open(select);
      }
      return;
    }
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        event.preventDefault();
        move(-1);
        break;
      case "Home":
        event.preventDefault();
        setActive(0, true);
        break;
      case "End":
        event.preventDefault();
        setActive(activeOptions.length - 1, true);
        break;
      case "Enter":
      case " ":
      case "Spacebar":
        event.preventDefault();
        choose(activeIndex);
        break;
      case "Escape":
        event.preventDefault();
        close(true);
        break;
      case "Tab":
        close();
        break;
      default:
        typeaheadMatch(event.key);
        break;
    }
  }

  function enhance(select) {
    if (select.dataset.cselect === "1") return;
    select.dataset.cselect = "1";

    const wrapper = document.createElement("div");
    wrapper.className = "cselect";
    for (const name of ["translation", "lang-select", "book-jump"]) {
      if (select.classList.contains(name)) wrapper.classList.add(name);
    }

    const trigger = document.createElement("button");
    trigger.type = "button";
    trigger.className = "cselect-trigger";
    trigger.setAttribute("role", "combobox");
    trigger.setAttribute("aria-haspopup", "listbox");
    trigger.setAttribute("aria-expanded", "false");
    trigger.setAttribute("aria-label", labelFor(select));

    const value = document.createElement("span");
    value.className = "cselect-value";
    const chevron = document.createElement("span");
    chevron.className = "cselect-chevron";
    chevron.setAttribute("aria-hidden", "true");
    trigger.append(value, chevron);

    select.parentNode.insertBefore(wrapper, select);
    wrapper.append(select, trigger);
    select.classList.add("cselect-native");
    select.tabIndex = -1;
    select.setAttribute("aria-hidden", "true");

    select._cselectTrigger = trigger;
    trigger.addEventListener("click", () => {
      if (activeSelect === select) close();
      else open(select);
    });
    trigger.addEventListener("keydown", (event) => onTriggerKeydown(event, select));
    select.addEventListener("change", () => syncSelect(select));
    syncSelect(select);

    const field = select.closest("label.filter-field");
    if (field) {
      field.addEventListener("click", (event) => {
        if (!isElement(event.target)) return;
        if (event.target.closest(".cselect")) return;
        event.preventDefault();
        trigger.click();
      });
    }
  }

  function enhanceAll() {
    document.querySelectorAll("select:not([data-cselect])").forEach(enhance);
  }

  window.addEventListener("resize", () => {
    if (activeSelect) positionMenu();
  });
  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && activeSelect) close(true);
  });
  window.addEventListener("pageshow", () => {
    window.setTimeout(syncAll, 0);
  });

  document.addEventListener("click", (event) => {
    const target = isElement(event.target) ? event.target : null;
    if (target && target.closest("#filters-reset")) {
      window.setTimeout(syncAll, 0);
    }
  });
  window.addEventListener("aletheia:sync-selects", syncAll);

  const start = () => {
    enhanceAll();
    syncAll();
    // The Rust side sets translation/language from the URL after the first
    // paint; keep the labels correct even if that happens before the ready
    // event.
    window.setTimeout(syncAll, 700);
    window.setTimeout(syncAll, 2200);
    window.setTimeout(syncAll, 4200);
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start, { once: true });
  } else {
    start();
  }

  // Leptos mounts the app after this script runs. Enhance the controls as soon
  // as they exist, then stop watching after a short settle window.
  const observer = new MutationObserver(enhanceAll);
  observer.observe(document.documentElement, { childList: true, subtree: true });
  window.setTimeout(() => {
    enhanceAll();
    observer.disconnect();
  }, 2500);

  // A tiny public hook for diagnostics and programmatic resets.
  window.__aletheiaSyncSelects = syncAll;
})();
