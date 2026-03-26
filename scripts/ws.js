// HomeRoute WebSocket client — updates DOM elements with data-ws-target attributes.
//
// Simple events (flat):
//   data-ws-target="servers:status:status:<id>"
//   Message: { type: "servers:status", data: { serverId: "<id>", status: "online" } }
//
// Agent events (nested data with appId):
//   data-ws-target="agent:metrics:cpuPercent:<appId>"
//   Message: { type: "agent:metrics", data: { appId: "<id>", cpuPercent: 35.5, ... } }
//
//   data-ws-target="agent:status:status:<appId>"
//   Message: { type: "agent:status", data: { appId: "<id>", status: "connected" } }
(function () {
  var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  var url = proto + '//' + location.host + '/api/ws';
  var ws, timer;

  function formatBytes(bytes) {
    if (bytes >= 1073741824) return (bytes / 1073741824).toFixed(1) + ' GB';
    if (bytes >= 1048576) return Math.round(bytes / 1048576) + ' MB';
    if (bytes >= 1024) return Math.round(bytes / 1024) + ' KB';
    return bytes + ' B';
  }

  function formatCpu(val) {
    return Math.round(val) + '%';
  }

  // Map service status to badge classes
  function serviceBadgeClass(status) {
    if (status === 'running') return 'bg-green-500/20 text-green-400';
    if (status === 'starting' || status === 'stopping') return 'bg-blue-500/20 text-blue-400';
    return 'bg-gray-500/20 text-gray-500';
  }

  function serviceLabel(field, status) {
    var prefix = '';
    if (field === 'codeServerStatus') prefix = 'IDE ';
    else if (field === 'appStatus') prefix = 'App ';
    else if (field === 'dbStatus') prefix = 'DB ';
    if (status === 'running') return prefix + 'ON';
    if (status === 'starting' || status === 'stopping') return prefix + '...';
    return prefix + 'OFF';
  }

  // Update a single element with a value from a WS message
  function updateElement(el, field, value) {
    // Special formatting for known fields
    if (field === 'cpuPercent') {
      el.textContent = formatCpu(value);
      // Color-code CPU
      el.className = el.className.replace(/text-\S+-\d+/g, '');
      if (value > 80) el.classList.add('text-red-400');
      else if (value > 50) el.classList.add('text-yellow-400');
      else el.classList.add('text-green-400');
      return;
    }
    if (field === 'memoryBytes') {
      el.textContent = formatBytes(value);
      return;
    }
    // Service status fields — update text + badge colors
    if (field === 'codeServerStatus' || field === 'appStatus' || field === 'dbStatus') {
      el.textContent = serviceLabel(field, value);
      el.className = el.className.replace(/bg-\S+\/20 text-\S+/g, '');
      var cls = serviceBadgeClass(value).split(' ');
      cls.forEach(function (c) { el.classList.add(c); });
      return;
    }
    // Status field (agent:status) — update text + badge
    if (field === 'status') {
      var labels = { connected: 'Connecté', deploying: 'Déploiement', pending: 'En attente', disconnected: 'Déconnecté', error: 'Erreur' };
      el.textContent = labels[value] || value;
      el.className = el.className.replace(/bg-\S+\/20 text-\S+/g, '');
      if (value === 'connected') { el.classList.add('bg-green-500/20', 'text-green-400'); }
      else if (value === 'deploying') { el.classList.add('bg-blue-500/20', 'text-blue-400'); }
      else if (value === 'pending') { el.classList.add('bg-yellow-500/20', 'text-yellow-400'); }
      else { el.classList.add('bg-red-500/20', 'text-red-400'); }
      return;
    }
    // Default: just set text
    el.textContent = String(value);
  }

  function handleMessage(ev) {
    try {
      var msg = JSON.parse(ev.data);
      if (!msg.type) return;

      var msgType = msg.type; // e.g. "agent:metrics", "agent:status", "servers:status"

      // Find all elements targeting this message type
      var targets = document.querySelectorAll('[data-ws-target^="' + msgType + ':"]');
      if (targets.length === 0) return;

      // Determine where the actual data lives
      var payload = msg.data || msg;
      // Determine the ID field for matching
      var idValue = payload.appId || payload.serverId || payload.id || null;

      targets.forEach(function (el) {
        var attr = el.getAttribute('data-ws-target');
        // Format: "type:field:id" e.g. "agent:metrics:cpuPercent:abc-123"
        // We need to parse carefully since the type itself may contain colons
        // Strip the msgType prefix + colon to get "field:id"
        var remainder = attr.substring(msgType.length + 1);
        var sepIdx = remainder.indexOf(':');
        var field, targetId;
        if (sepIdx >= 0) {
          field = remainder.substring(0, sepIdx);
          targetId = remainder.substring(sepIdx + 1);
        } else {
          field = remainder;
          targetId = null;
        }

        // Match on ID if specified
        if (targetId && idValue && targetId !== idValue) return;

        // Get the value from payload
        var value = payload[field];
        if (value === undefined) return;

        updateElement(el, field, value);
      });
    } catch (e) { /* ignore non-JSON messages */ }
  }

  function connect() {
    ws = new WebSocket(url);
    ws.onmessage = handleMessage;
    ws.onclose = function () { timer = setTimeout(connect, 3000); };
    ws.onerror = function () { ws.close(); };
  }

  // Only connect if there are ws-target elements on the page
  if (document.querySelector('[data-ws-target]')) {
    connect();
  }
})();
