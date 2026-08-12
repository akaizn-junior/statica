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
          .catch(() => []),
      );
    }
    return cache.get(url);
  };

  const text = (value) => String(value || "").toLowerCase();
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
    const trigger = document.querySelector(`[aria-controls="${modal.id}"]`);
    const input = modal.querySelector("input[type='search']");
    const results = modal.querySelector("[data-statica-search-results]");
    const limit = Number(modal.dataset.limit || 10);
    const indexUrl = modal.dataset.index || "/search.json";

    trigger?.addEventListener("click", () => {
      modal.showModal();
      input?.focus();
    });

    modal
      .querySelector("[data-statica-search-close]")
      ?.addEventListener("click", () => modal.close());

    input?.addEventListener("input", async () => {
      const terms = input.value
        .trim()
        .toLowerCase()
        .split(/\s+/)
        .filter(Boolean);

      if (!terms.length) {
        results.replaceChildren();
        return;
      }

      const index = await load(indexUrl);
      const matches = index
        .map((item) => ({ item, rank: score(item, terms) }))
        .filter((match) => match.rank > 0)
        .sort((a, b) => b.rank - a.rank)
        .slice(0, limit);

      results.replaceChildren(
        ...matches.map(({ item }) => {
          const link = document.createElement("a");
          link.className = "statica-search-result";
          link.href = item.url;

          const title = document.createElement("strong");
          title.textContent = item.title || item.url;

          const excerpt = document.createElement("span");
          excerpt.textContent = item.excerpt || item.url;

          link.append(title, excerpt);
          return link;
        }),
      );
    });
  }
};

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", ready);
} else {
  ready();
}
