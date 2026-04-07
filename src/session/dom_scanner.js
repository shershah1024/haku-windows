// DOM Scanner — injected into browser tabs to discover interactive elements.
// Cross-platform: pure JavaScript, works in any browser.
// This is a simplified version; the full ScriptInjector.swift JS is much larger.

(function() {
    var tools = [];
    var seen = new Set();

    function slugify(text) {
        return text.toLowerCase()
            .replace(/[^a-z0-9]+/g, '_')
            .replace(/^_|_$/g, '')
            .substring(0, 40);
    }

    function bestLabel(el) {
        return el.getAttribute('aria-label')
            || el.getAttribute('aria-labelledby') && document.getElementById(el.getAttribute('aria-labelledby'))?.textContent?.trim()
            || el.textContent?.trim()?.substring(0, 50)
            || el.getAttribute('title')
            || el.getAttribute('placeholder')
            || el.getAttribute('name')
            || el.id
            || '';
    }

    function cssPath(el) {
        var path = [];
        while (el && el.nodeType === 1) {
            var selector = el.nodeName.toLowerCase();
            if (el.id) {
                selector += '#' + el.id;
                path.unshift(selector);
                break;
            }
            var sib = el, nth = 1;
            while (sib = sib.previousElementSibling) {
                if (sib.nodeName === el.nodeName) nth++;
            }
            if (nth > 1) selector += ':nth-of-type(' + nth + ')';
            path.unshift(selector);
            el = el.parentNode;
        }
        return path.join(' > ');
    }

    function isVisible(el) {
        if (!el.offsetParent && el.style?.display !== 'fixed') return false;
        var style = window.getComputedStyle(el);
        return style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0';
    }

    // Buttons
    document.querySelectorAll('button, [role="button"], input[type="submit"], input[type="button"], a[role="button"]').forEach(function(el) {
        if (!isVisible(el)) return;
        var label = bestLabel(el);
        if (!label || seen.has(label)) return;
        seen.add(label);
        tools.push({name: 'click_' + slugify(label), description: "Click '" + label + "' button", type: 'click', selector: cssPath(el)});
    });

    // Input fields
    document.querySelectorAll('input[type="text"], input[type="email"], input[type="password"], input[type="search"], input[type="url"], input[type="tel"], input[type="number"], textarea, [contenteditable="true"]').forEach(function(el) {
        if (!isVisible(el)) return;
        var label = el.getAttribute('aria-label') || el.getAttribute('placeholder') || el.getAttribute('name') || el.id || '';
        if (!label || seen.has(label)) return;
        seen.add(label);
        tools.push({name: 'fill_' + slugify(label), description: "Fill '" + label + "' field. Current: '" + (el.value || '').substring(0, 50) + "'", type: 'fill', selector: cssPath(el)});
    });

    // Select dropdowns
    document.querySelectorAll('select').forEach(function(el) {
        if (!isVisible(el)) return;
        var label = el.getAttribute('aria-label') || el.getAttribute('name') || el.id || '';
        if (!label || seen.has(label)) return;
        seen.add(label);
        tools.push({name: 'select_' + slugify(label), description: "Select from '" + label + "' dropdown", type: 'select', selector: cssPath(el)});
    });

    // Links
    document.querySelectorAll('a[href]').forEach(function(el) {
        if (!isVisible(el)) return;
        var label = bestLabel(el);
        if (!label || label.length < 2 || seen.has(label)) return;
        seen.add(label);
        tools.push({name: 'click_' + slugify(label), description: "Click '" + label + "' link", type: 'click', selector: cssPath(el)});
    });

    // ARIA widgets
    document.querySelectorAll('[role="checkbox"], [role="switch"], [role="tab"], [role="menuitem"], [role="option"], [role="radio"]').forEach(function(el) {
        if (!isVisible(el)) return;
        var label = bestLabel(el);
        if (!label || seen.has(label)) return;
        seen.add(label);
        var prefix = el.getAttribute('role') === 'checkbox' || el.getAttribute('role') === 'switch' ? 'toggle' : 'click';
        tools.push({name: prefix + '_' + slugify(label), description: prefix.charAt(0).toUpperCase() + prefix.slice(1) + " '" + label + "'", type: 'click', selector: cssPath(el)});
    });

    // Page content
    var content = '';
    var mainEl = document.querySelector('main, [role="main"], article, .content, #content');
    if (mainEl) {
        content = mainEl.textContent.trim().substring(0, 2000);
    }

    var headings = [];
    document.querySelectorAll('h1, h2, h3').forEach(function(el) {
        headings.push({level: parseInt(el.tagName[1]), text: el.textContent.trim()});
    });

    return JSON.stringify({
        url: window.location.href,
        title: document.title,
        toolCount: tools.length,
        tools: tools.slice(0, 200),
        headings: headings.slice(0, 20),
        content: content
    });
})()
