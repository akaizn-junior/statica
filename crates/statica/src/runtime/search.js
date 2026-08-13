const label = (value) =>
  String(value || "")
    .replace(/^statica:/, "")
    .replace(/[:_-]+/g, " ")
    .replace(/\b\w/g, (match) => match.toUpperCase());

const ready = async () => {
  const modals = document.querySelectorAll("[data-statica-search]");
  if (!modals.length) return;

  const cache = new Map();
  const load = async (url) => {
    if (!cache.has(url)) {
      cache.set(
        url,
        fetch(url)
          .then((response) => (response.ok ? response.json() : []))
          .then((data) => (Array.isArray(data) ? data : []))
          .catch(() => []),
      );
    }
    return cache.get(url);
  };

  const text = (value) => String(value || "").toLowerCase();
  const valueAt = (item, path) =>
    String(path || "url")
      .split(".")
      .filter(Boolean)
      .reduce((current, part) => current?.[part], item);
  const score = (item, terms) =>
    terms.reduce(
      (total, term) =>
        total +
        (text(item.title).includes(term) ? 8 : 0) +
        (text(item.text).includes(term) ? 2 : 0) +
        (text(item.url).includes(term) ? 1 : 0),
      0,
    );

  for (const modal of modals) {
    if (modal.dataset.staticaSearchReady === "true") continue;
    modal.dataset.staticaSearchReady = "true";

    const trigger =
      modal
        .closest(".statica-search")
        ?.querySelector(`[aria-controls="${modal.id}"]`) ||
      document.querySelector(`[aria-controls="${modal.id}"]`);
    const input = modal.querySelector("input[type='search']");
    const meta = modal.querySelector("[data-statica-search-meta]");
    const results = modal.querySelector("[data-statica-search-results]");
    const limit = Number(modal.dataset.limit || 10);
    const indexUrl = modal.dataset.index || "/search.json";
    const urlField = modal.dataset.urlField || "url";
    const filters = String(modal.dataset.filters || "")
      .split(",")
      .map((filter) => filter.trim())
      .filter(Boolean);
    let activeIndex = -1;
    const resultLinks = () => [...results.querySelectorAll(".statica-search-result")];
    const setActive = (next) => {
      const links = resultLinks();
      if (!links.length) {
        activeIndex = -1;
        return;
      }
      activeIndex = (next + links.length) % links.length;
      links.forEach((link, index) => {
        const active = index === activeIndex;
        link.toggleAttribute("data-active", active);
        link.setAttribute("aria-selected", active ? "true" : "false");
        if (active) link.scrollIntoView({ block: "nearest" });
      });
    };
    const close = () => {
      if (modal.open) modal.close();
    };

    if (!input || !meta || !results) continue;

    if (filters.length) {
      meta.replaceChildren(filterList(filters));
    }

    trigger?.addEventListener("click", () => {
      if (!modal.open) modal.showModal();
      input.focus();
    });

    modal.addEventListener("cancel", close);
    modal.addEventListener("click", (event) => {
      if (event.target !== modal) return;
      const rect = modal.getBoundingClientRect();
      const outside =
        event.clientX < rect.left ||
        event.clientX > rect.right ||
        event.clientY < rect.top ||
        event.clientY > rect.bottom;
      if (outside) close();
    });

    modal
      .querySelector("[data-statica-search-close]")
      ?.addEventListener("click", close);

    meta.addEventListener("click", (event) => {
      const button = event.target.closest("[data-filter]");
      if (!button || !meta.contains(button)) return;
      const filter = String(button.dataset.filter || "").trim();
      if (!filter) return;
      const terms = input.value
        .trim()
        .toLowerCase()
        .split(/\s+/)
        .filter(Boolean);
      if (!terms.includes(filter.toLowerCase())) {
        terms.push(filter);
      }
      input.value = terms.join(" ");
      input.focus();
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });

    input.addEventListener("input", async () => {
      const terms = input.value
        .trim()
        .toLowerCase()
        .split(/\s+/)
        .filter(Boolean);

      if (!terms.length) {
        activeIndex = -1;
        results.replaceChildren();
        meta.replaceChildren(...(filters.length ? [filterList(filters)] : []));
        return;
      }

      const index = await load(indexUrl);
      const matches = index
        .map((item) => ({ item, rank: score(item, terms) }))
        .filter((match) => match.rank > 0)
        .sort((a, b) => b.rank - a.rank)
        .slice(0, limit);

      meta.replaceChildren(
        summary(
          `${matches.length} result${matches.length === 1 ? "" : "s"}`,
          filters.length ? filterList(filters) : indexUrl,
        ),
      );

      results.replaceChildren(
        ...matches.map(({ item }) => {
          const href = String(valueAt(item, urlField) || item.url || "#");
          const link = document.createElement("a");
          link.className = "statica-search-result";
          link.href = href;
          link.setAttribute("role", "option");

          const body = document.createElement("span");
          body.className = "statica-search-result-body";

          const title = document.createElement("strong");
          title.textContent = item.title || item.url;

          const excerpt = document.createElement("span");
          excerpt.className = "statica-search-excerpt";
          excerpt.textContent = item.excerpt || item.url;

          const metadata = document.createElement("small");
          metadata.className = "statica-search-result-meta";
          metadata.append(chip(item.section || "page"), chip(href));

          for (const field of (item.meta || []).slice(0, 3)) {
            metadata.append(chip(`${label(field.name)}: ${shorten(field.value, 64)}`));
          }

          body.append(title, excerpt, metadata);
          link.append(body);
          return link;
        }),
      );
      setActive(matches.length ? 0 : -1);
    });

    input.addEventListener("keydown", (event) => {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActive(activeIndex + 1);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setActive(activeIndex - 1);
      } else if (event.key === "Enter") {
        const active = resultLinks()[activeIndex];
        if (active) {
          event.preventDefault();
          active.click();
        }
      } else if (event.key === "Escape") {
        close();
      }
    });
  }
};

const chip = (value) => {
  const span = document.createElement("span");
  span.textContent = value;
  return span;
};

const filterList = (filters) => {
  const list = document.createElement("span");
  list.className = "statica-search-filters";
  for (const filter of filters) {
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.filter = filter;
    button.textContent = label(filter);
    list.append(button);
  }
  return list;
};

const shorten = (value, max) => {
  const text = String(value || "");
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
};

const summary = (count, detail) => {
  const p = document.createElement("p");
  const strong = document.createElement("strong");
  strong.textContent = count;
  const span = document.createElement("span");
  if (detail instanceof Node) {
    span.append(detail);
  } else {
    span.textContent = detail;
  }
  p.append(strong, span);
  return p;
};

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", ready);
} else {
  ready();
}
