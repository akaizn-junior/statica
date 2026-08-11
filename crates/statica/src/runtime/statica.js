/**
 * statica.js — scoped DOM helpers for fragment actions.
 *
 * Fragment scripts receive a scoped `document` object for the current
 * fragment instance (`data-s="id-hash"`). Authors write normal DOM calls like
 * `document.querySelector(".btn")`; build inlines this helper.
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
    var root = host || document;

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
      body: document.body,
      currentScript: scriptEl,
      host: host,
      querySelector: qs,
      querySelectorAll: qsa,
      addEventListener: function () {
        return host && host.addEventListener.apply(host, arguments);
      },
      createElement: function () {
        return document.createElement.apply(document, arguments);
      },
      execCommand: function () {
        return document.execCommand.apply(document, arguments);
      },
      getElementById: function (id) {
        return qs("#" + id);
      },
    };
  }

  function runScoped(scriptEl, scopeId, action) {
    return action(createScope(scriptEl, scopeId));
  }

  return {
    run: runScoped,
  };
});
