/* Tasty site — progressive enhancement only. Every page is usable with JS off. */
(function () {
  "use strict";

  var root = document.documentElement;
  var body = document.body;

  /* ------------------------------------------------------------- theme */

  function applyTheme(name) {
    if (name === "light" || name === "dark") {
      root.setAttribute("data-theme", name);
    } else {
      root.removeAttribute("data-theme");
    }
    try { localStorage.setItem("tasty-theme", name); } catch (e) { /* private mode */ }
  }

  function currentTheme() {
    var attr = root.getAttribute("data-theme");
    if (attr) return attr;
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }

  var themeBtn = document.querySelector(".theme-toggle");
  if (themeBtn) {
    themeBtn.addEventListener("click", function () {
      applyTheme(currentTheme() === "light" ? "dark" : "light");
    });
  }

  /* --------------------------------------------------- mobile nav drawer */

  var menuBtn = document.querySelector(".menu-btn");
  if (menuBtn) {
    menuBtn.addEventListener("click", function () {
      var open = body.hasAttribute("data-nav-open");
      if (open) body.removeAttribute("data-nav-open");
      else body.setAttribute("data-nav-open", "");
      menuBtn.setAttribute("aria-expanded", String(!open));
    });
    document.addEventListener("click", function (e) {
      if (!body.hasAttribute("data-nav-open")) return;
      if (e.target.closest(".sidebar") || e.target.closest(".menu-btn")) return;
      body.removeAttribute("data-nav-open");
      menuBtn.setAttribute("aria-expanded", "false");
    });
  }

  /* ------------------------------------------------------- code copying */

  document.querySelectorAll(".code-block").forEach(function (block) {
    var pre = block.querySelector("pre");
    if (!pre) return;
    var btn = document.createElement("button");
    btn.className = "copy-btn";
    btn.type = "button";
    btn.textContent = block.dataset.copyLabel || "copy";
    btn.addEventListener("click", function () {
      var text = pre.innerText;
      var done = function () {
        btn.textContent = block.dataset.copiedLabel || "copied";
        btn.setAttribute("data-copied", "");
        setTimeout(function () {
          btn.textContent = block.dataset.copyLabel || "copy";
          btn.removeAttribute("data-copied");
        }, 1400);
      };
      if (navigator.clipboard) {
        navigator.clipboard.writeText(text).then(done, function () {});
      } else {
        var ta = document.createElement("textarea");
        ta.value = text;
        ta.style.position = "fixed";
        ta.style.opacity = "0";
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand("copy"); done(); } catch (e) { /* ignore */ }
        document.body.removeChild(ta);
      }
    });
    block.appendChild(btn);
  });

  /* ------------------------------------------------------- toc scrollspy */

  var tocLinks = Array.prototype.slice.call(document.querySelectorAll(".toc a"));
  if (tocLinks.length && "IntersectionObserver" in window) {
    var byId = {};
    tocLinks.forEach(function (a) {
      var id = decodeURIComponent(a.getAttribute("href") || "").replace(/^#/, "");
      if (id) byId[id] = a;
    });
    var visible = new Set();
    var observer = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) visible.add(entry.target.id);
        else visible.delete(entry.target.id);
      });
      var first = null;
      Object.keys(byId).some(function (id) {
        if (visible.has(id)) { first = id; return true; }
        return false;
      });
      tocLinks.forEach(function (a) { a.removeAttribute("data-active"); });
      if (first && byId[first]) byId[first].setAttribute("data-active", "");
    }, { rootMargin: "-72px 0px -70% 0px", threshold: 0 });

    Object.keys(byId).forEach(function (id) {
      var el = document.getElementById(id);
      if (el) observer.observe(el);
    });
  }

  /* ------------------------------------------------------------- search */

  var searchInput = document.querySelector(".search input");
  var searchResults = document.querySelector(".search__results");
  if (searchInput && searchResults) {
    var index = null;
    var loading = false;
    var activeIdx = -1;

    function loadIndex() {
      if (index || loading) return;
      loading = true;
      fetch(searchInput.dataset.index)
        .then(function (r) { return r.json(); })
        .then(function (json) { index = json; loading = false; run(); })
        .catch(function () { loading = false; });
    }

    function score(entry, terms) {
      var title = entry.t.toLowerCase();
      var crumb = (entry.c || "").toLowerCase();
      var heads = (entry.h || "").toLowerCase();
      var total = 0;
      for (var i = 0; i < terms.length; i++) {
        var q = terms[i];
        var s = 0;
        if (title === q) s = 100;
        else if (title.indexOf(q) === 0) s = 60;
        else if (title.indexOf(q) !== -1) s = 40;
        else if (crumb.indexOf(q) !== -1) s = 18;
        else if (heads.indexOf(q) !== -1) s = 10;
        if (s === 0) return 0;
        total += s;
      }
      return total;
    }

    function run() {
      var raw = searchInput.value.trim().toLowerCase();
      if (!raw) { close(); return; }
      if (!index) { loadIndex(); return; }
      var terms = raw.split(/\s+/);
      var hits = [];
      for (var i = 0; i < index.length; i++) {
        var s = score(index[i], terms);
        if (s > 0) hits.push({ e: index[i], s: s });
      }
      hits.sort(function (a, b) { return b.s - a.s; });
      hits = hits.slice(0, 24);

      searchResults.innerHTML = "";
      if (!hits.length) {
        var empty = document.createElement("div");
        empty.className = "search__empty";
        empty.textContent = searchInput.dataset.emptyLabel || "No results";
        searchResults.appendChild(empty);
      } else {
        hits.forEach(function (hit) {
          var a = document.createElement("a");
          a.href = searchInput.dataset.base + hit.e.u;
          a.textContent = hit.e.t;
          var crumb = document.createElement("span");
          crumb.className = "r-crumb";
          crumb.textContent = hit.e.c || "";
          a.appendChild(crumb);
          searchResults.appendChild(a);
        });
      }
      activeIdx = -1;
      searchResults.setAttribute("data-open", "");
    }

    function close() {
      searchResults.removeAttribute("data-open");
      activeIdx = -1;
    }

    function items() {
      return Array.prototype.slice.call(searchResults.querySelectorAll("a"));
    }

    function move(delta) {
      var list = items();
      if (!list.length) return;
      list.forEach(function (a) { a.removeAttribute("data-active"); });
      activeIdx = (activeIdx + delta + list.length) % list.length;
      list[activeIdx].setAttribute("data-active", "");
      list[activeIdx].scrollIntoView({ block: "nearest" });
    }

    searchInput.addEventListener("focus", loadIndex);
    searchInput.addEventListener("input", run);
    searchInput.addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown") { e.preventDefault(); move(1); }
      else if (e.key === "ArrowUp") { e.preventDefault(); move(-1); }
      else if (e.key === "Enter") {
        var list = items();
        if (activeIdx >= 0 && list[activeIdx]) { e.preventDefault(); list[activeIdx].click(); }
      } else if (e.key === "Escape") { close(); searchInput.blur(); }
    });
    document.addEventListener("click", function (e) {
      if (!e.target.closest(".search")) close();
    });
    document.addEventListener("keydown", function (e) {
      if (e.key === "/" && document.activeElement !== searchInput &&
          !/^(INPUT|TEXTAREA)$/.test(document.activeElement.tagName)) {
        e.preventDefault();
        searchInput.focus();
      }
    });
  }

  /* -------------------------------------------- keep active nav in view */

  var current = document.querySelector('.nav-list a[aria-current="page"]');
  if (current) {
    var sidebar = document.querySelector(".sidebar");
    if (sidebar && window.matchMedia("(min-width: 901px)").matches) {
      var top = current.offsetTop - sidebar.clientHeight / 3;
      if (top > 0) sidebar.scrollTop = top;
    }
  }
})();
