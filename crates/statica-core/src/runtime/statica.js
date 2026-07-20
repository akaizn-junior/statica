/**
 * statica.js — scoped DOM helpers for fragment actions.
 *
 * `$` scopes selectors to the fragment instance (`data-s="id-hash"`), then selects.
 * Not jQuery. Authors write `$.querySelector(".btn")`; build inlines this helper.
 */
(function (root, factory) {
  if (typeof module === "object" && module.exports) {
    module.exports = factory();
  } else {
    root.__statica = factory();
  }
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  function scopeSelector(sel, scopeId) {
    return String(sel)
      .split(",")
      .map(function (part) {
        part = part.trim();
        if (!part) return part;
        if (part.indexOf('[data-s="') !== -1) return part;
        return part + '[data-s="' + scopeId + '"]';
      })
      .join(", ");
  }

  function findHost(scriptEl, scopeId) {
    var n = scriptEl && scriptEl.previousElementSibling;
    while (n) {
      if (n.getAttribute && n.getAttribute("data-s") === scopeId) return n;
      n = n.previousElementSibling;
    }
    return document.querySelector('[data-s="' + scopeId + '"]');
  }

  function createScope(scriptEl, scopeId) {
    var host = findHost(scriptEl, scopeId);
    var root = (host && host.parentNode) || document;

    function qs(sel) {
      var scoped = scopeSelector(sel, scopeId);
      if (host && host.matches && host.matches(scoped)) return host;
      return root.querySelector(scoped);
    }

    function qsa(sel) {
      var scoped = scopeSelector(sel, scopeId);
      var list = Array.prototype.slice.call(root.querySelectorAll(scoped));
      if (host && host.matches && host.matches(scoped) && list.indexOf(host) === -1) {
        list.unshift(host);
      }
      return list;
    }

    return {
      host: host,
      querySelector: qs,
      querySelectorAll: qsa,
      addEventListener: function () {
        return host && host.addEventListener.apply(host, arguments);
      },
      getElementById: function (id) {
        return qs("#" + id);
      },
    };
  }

  return {
    scope: createScope,
    /** @deprecated use scope() — kept for inlined build output */
    __staticaScope: createScope,
  };
});
