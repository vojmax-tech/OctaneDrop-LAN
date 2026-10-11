(function () {
  const input = document.getElementById('file-input');
  const dropzone = document.getElementById('dropzone');
  const selectedFiles = document.getElementById('selected-files');
  const clearSelectionButton = document.getElementById('clear-selection');
  const lanUrl = document.getElementById('lan-url');
  const dashboardLayout = document.querySelector('.dashboard-layout-refined');
  const sidebarToggleButton = document.getElementById('sidebar-toggle');
  const filesCard = document.querySelector('.files-card');
  const bulkDownloadButton = document.getElementById('download-selected');
  const selectAllFilesCheckbox = document.getElementById('select-all-files');
  const copyFeedback = document.getElementById('copy-feedback');
  const uploadForm = document.getElementById('upload-form');
  const uploadButton = uploadForm?.querySelector('button[type="submit"]');
  const liveStatus = document.getElementById('live-status');
  const liveDot = document.getElementById('live-dot');
  const toastStack = document.getElementById('toast-stack');
  const peerPanel = document.getElementById('peer-panel');
  const peerSelect = document.getElementById('peer-select');
  const peerStatusLabel = document.getElementById('peer-status-label');
  const peerDetail = document.getElementById('peer-detail');
  const peerSessionList = document.getElementById('peer-session-list');

  let liveSignature = filesCard?.dataset.filesSignature || '';
  let uploadPending = false;
  let refreshQueued = false;
  let socket = null;
  let reconnectTimer = null;
  let reconnectAttempt = 0;
  let pingTimer = null;
  let peerPollTimer = null;
  let transferPollTimer = null;
  const sidebarPreferenceKey = 'voltdrop.sidebar.expanded';
  const runMode = (filesCard?.dataset.runMode || peerPanel?.dataset.runMode || '').toLowerCase();
  const isP2PMode = runMode === 'p2p';
  const trackedTransfers = new Map();
  const selectedFileIds = new Set();

  function showToast(message, kind = 'success', durationMs = 2600) {
    if (!toastStack || !message) {
      return;
    }

    const toast = document.createElement('div');
    toast.className = `toast toast-${kind}`;
    toast.role = 'status';
    toast.textContent = message;
    toastStack.appendChild(toast);

    // Trigger transition after insertion.
    window.requestAnimationFrame(() => {
      toast.classList.add('is-visible');
    });

    window.setTimeout(() => {
      toast.classList.remove('is-visible');
      toast.classList.add('is-leaving');
      window.setTimeout(() => toast.remove(), 260);
    }, durationMs);
  }

  function hydrateInitialToast() {
    if (!toastStack) {
      return;
    }
    const initialToast = toastStack.querySelector('.initial-toast');
    if (!initialToast) {
      return;
    }
    window.requestAnimationFrame(() => {
      initialToast.classList.add('is-visible');
    });
    window.setTimeout(() => {
      initialToast.classList.remove('is-visible');
      initialToast.classList.add('is-leaving');
      window.setTimeout(() => initialToast.remove(), 260);
    }, 2600);
  }

  function describeFiles(fileList) {
    if (!fileList || !fileList.length) {
      return 'No files selected.';
    }
    const names = Array.from(fileList).slice(0, 3).map(file => file.name);
    const remaining = fileList.length - names.length;
    return remaining > 0 ? `${names.join(', ')} + ${remaining} more` : names.join(', ');
  }

  function hasPendingLocalSelection() {
    return Boolean(input?.files?.length) && !uploadPending;
  }

  function updateSelectionUi() {
    if (selectedFiles) {
      selectedFiles.textContent = describeFiles(input?.files);
    }
    if (clearSelectionButton) {
      clearSelectionButton.classList.toggle('hidden', !input?.files?.length || uploadPending);
    }
  }

  function setLiveState(state, text) {
    if (liveStatus) {
      liveStatus.textContent = text;
    }
    if (liveDot) {
      liveDot.dataset.state = state;
    }
  }

  function reloadOrQueue(signature) {
    if (signature) {
      liveSignature = signature;
      filesCard.dataset.filesSignature = signature;
    }

    if (uploadPending || hasPendingLocalSelection()) {
      refreshQueued = true;
      setLiveState('waiting', 'Change detected. Refresh is waiting for your pending upload.');
      return;
    }

    window.location.reload();
  }

  function maybeRefreshQueued() {
    if (refreshQueued && !uploadPending && !hasPendingLocalSelection()) {
      window.location.reload();
    }
  }

  function clearReconnectTimer() {
    if (reconnectTimer) {
      window.clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  function clearPingTimer() {
    if (pingTimer) {
      window.clearInterval(pingTimer);
      pingTimer = null;
    }
  }

  function applyNetworkState(payload, { notify = false } = {}) {
    if (!payload || typeof payload !== 'object') {
      return;
    }

    const nextLanUrl = typeof payload.lan_url === 'string' ? payload.lan_url : '';
    if (lanUrl && nextLanUrl) {
      lanUrl.textContent = nextLanUrl;
    }

    if (!notify) {
      return;
    }

    const message = typeof payload.message === 'string' && payload.message
      ? payload.message
      : (nextLanUrl ? `Network changed. New LAN address: ${nextLanUrl}` : 'Network changed.');
    showToast(message, payload.has_lan === false ? 'error' : 'success', 4400);
    if (copyFeedback) {
      copyFeedback.textContent = message;
    }
  }

  function clearPeerPollTimer() {
    if (peerPollTimer) {
      window.clearInterval(peerPollTimer);
      peerPollTimer = null;
    }
  }

  function clearTransferPollTimer() {
    if (transferPollTimer) {
      window.clearInterval(transferPollTimer);
      transferPollTimer = null;
    }
  }

  function getFileSelectionInputs() {
    return Array.from(document.querySelectorAll('.file-select-checkbox'));
  }

  function renderBulkSelectionState() {
    const inputs = getFileSelectionInputs();
    const allIds = new Set();
    inputs.forEach(inputNode => {
      const fileId = String(inputNode.dataset.fileId || '').trim();
      if (fileId) {
        allIds.add(fileId);
      }
    });

    selectedFileIds.forEach(fileId => {
      if (!allIds.has(fileId)) {
        selectedFileIds.delete(fileId);
      }
    });

    const selectedCount = selectedFileIds.size;
    if (bulkDownloadButton) {
      bulkDownloadButton.disabled = selectedCount === 0;
      bulkDownloadButton.textContent = `Download Selected (${selectedCount})`;
    }

    if (!selectAllFilesCheckbox) {
      return;
    }
    if (allIds.size === 0) {
      selectAllFilesCheckbox.checked = false;
      selectAllFilesCheckbox.indeterminate = false;
      selectAllFilesCheckbox.disabled = true;
      return;
    }

    selectAllFilesCheckbox.disabled = false;
    selectAllFilesCheckbox.checked = selectedCount > 0 && selectedCount === allIds.size;
    selectAllFilesCheckbox.indeterminate = selectedCount > 0 && selectedCount < allIds.size;
  }

  function setFileSelection(fileId, selected) {
    const normalizedId = String(fileId || '').trim();
    if (!normalizedId) {
      return;
    }
    if (selected) {
      selectedFileIds.add(normalizedId);
    } else {
      selectedFileIds.delete(normalizedId);
    }
    getFileSelectionInputs().forEach(inputNode => {
      if (String(inputNode.dataset.fileId || '').trim() === normalizedId) {
        inputNode.checked = selected;
      }
    });
    renderBulkSelectionState();
  }

  function handleFileSelectionChange(event) {
    const target = event.target;
    if (!target || !(target instanceof HTMLInputElement)) {
      return;
    }
    if (!target.classList.contains('file-select-checkbox')) {
      return;
    }
    const fileId = target.dataset.fileId || '';
    setFileSelection(fileId, target.checked);
  }

  function handleSelectAllChange() {
    if (!selectAllFilesCheckbox) {
      return;
    }
    const shouldSelectAll = selectAllFilesCheckbox.checked;
    if (!shouldSelectAll) {
      selectedFileIds.clear();
      getFileSelectionInputs().forEach(inputNode => {
        inputNode.checked = false;
      });
      renderBulkSelectionState();
      return;
    }

    getFileSelectionInputs().forEach(inputNode => {
      const fileId = String(inputNode.dataset.fileId || '').trim();
      if (!fileId) {
        return;
      }
      inputNode.checked = true;
      selectedFileIds.add(fileId);
    });
    renderBulkSelectionState();
  }

  function parseDownloadFilename(dispositionHeader) {
    if (!dispositionHeader) {
      return '';
    }
    const utf8Match = dispositionHeader.match(/filename\*=UTF-8''([^;]+)/i);
    if (utf8Match && utf8Match[1]) {
      try {
        return decodeURIComponent(utf8Match[1]);
      } catch (_) {
        // Fall through to ASCII filename parsing.
      }
    }
    const basicMatch = dispositionHeader.match(/filename=\"?([^\";]+)\"?/i);
    return basicMatch && basicMatch[1] ? basicMatch[1].trim() : '';
  }

  async function handleBatchDownload() {
    if (!filesCard || !bulkDownloadButton) {
      return;
    }
    const csrfToken = filesCard.dataset.csrfToken;
    const fileIds = Array.from(selectedFileIds);
    if (!csrfToken || !fileIds.length) {
      renderBulkSelectionState();
      return;
    }

    bulkDownloadButton.disabled = true;
    const originalLabel = bulkDownloadButton.textContent;
    bulkDownloadButton.textContent = 'Preparing ZIP...';
    try {
      const response = await fetch('/api/files/batch-download', {
        method: 'POST',
        headers: {
          'X-CSRF-Token': csrfToken,
          'Accept': 'application/zip, application/json',
          'Content-Type': 'application/json'
        },
        credentials: 'same-origin',
        body: JSON.stringify({ file_ids: fileIds })
      });

      if (!response.ok) {
        let errorMessage = 'Could not prepare selected files.';
        const contentType = (response.headers.get('Content-Type') || '').toLowerCase();
        if (contentType.includes('application/json')) {
          const payload = await response.json();
          errorMessage = payload.error || errorMessage;
        } else {
          const text = await response.text();
          if (text) {
            errorMessage = text;
          }
        }
        throw new Error(errorMessage);
      }

      const archiveBlob = await response.blob();
      if (!archiveBlob.size) {
        throw new Error('The generated ZIP archive was empty.');
      }

      const filenameFromHeader = parseDownloadFilename(response.headers.get('Content-Disposition') || '');
      const archiveFilename = filenameFromHeader || `voltdrop-files-${Date.now()}.zip`;
      const downloadUrl = window.URL.createObjectURL(archiveBlob);
      const anchor = document.createElement('a');
      anchor.href = downloadUrl;
      anchor.download = archiveFilename;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      window.URL.revokeObjectURL(downloadUrl);

      const archivedCount = Number(response.headers.get('X-Archive-Count') || fileIds.length);
      const skippedCount = Number(response.headers.get('X-Archive-Skipped') || 0);
      const successMessage = skippedCount > 0
        ? `Downloading ${archivedCount} file(s) as ZIP. ${skippedCount} file(s) were skipped.`
        : `Downloading ${archivedCount} file(s) as ZIP.`;
      if (copyFeedback) {
        copyFeedback.textContent = successMessage;
        window.setTimeout(() => {
          if (copyFeedback.textContent === successMessage) {
            copyFeedback.textContent = '\u00A0';
          }
        }, 2800);
      }
      showToast(successMessage, 'success', 2400);

      selectedFileIds.clear();
      getFileSelectionInputs().forEach(inputNode => {
        inputNode.checked = false;
      });
      renderBulkSelectionState();
    } catch (error) {
      const errorMessage = error.message || 'Could not download selected files.';
      if (copyFeedback) {
        copyFeedback.textContent = errorMessage;
        window.setTimeout(() => {
          if (copyFeedback.textContent === errorMessage) {
            copyFeedback.textContent = '\u00A0';
          }
        }, 3600);
      }
      showToast(errorMessage, 'error', 3200);
    } finally {
      bulkDownloadButton.textContent = originalLabel || 'Download Selected';
      renderBulkSelectionState();
    }
  }

  function setSidebarExpanded(expanded, { persist = true } = {}) {
    if (!dashboardLayout || !sidebarToggleButton) {
      return;
    }

    const isExpanded = Boolean(expanded);
    dashboardLayout.classList.toggle('sidebar-expanded', isExpanded);
    sidebarToggleButton.setAttribute('aria-expanded', isExpanded ? 'true' : 'false');
    sidebarToggleButton.textContent = isExpanded ? 'Hide Sidebar Details' : 'Show Sidebar Details';

    if (!persist) {
      return;
    }
    try {
      window.localStorage.setItem(sidebarPreferenceKey, isExpanded ? '1' : '0');
    } catch (_) {
      // Ignore persistence errors.
    }
  }

  function initSidebarToggle() {
    if (!dashboardLayout || !sidebarToggleButton) {
      return;
    }

    let initialExpanded = false;
    try {
      initialExpanded = window.localStorage.getItem(sidebarPreferenceKey) === '1';
    } catch (_) {
      initialExpanded = false;
    }

    setSidebarExpanded(initialExpanded, { persist: false });

    sidebarToggleButton.addEventListener('click', () => {
      const expandedNow = dashboardLayout.classList.contains('sidebar-expanded');
      setSidebarExpanded(!expandedNow);
    });
  }

  function startPingTimer() {
    clearPingTimer();
    pingTimer = window.setInterval(() => {
      if (socket && socket.readyState === WebSocket.OPEN) {
        try {
          socket.send('ping');
        } catch (_) {
          // Ignore transient socket issues.
        }
      }
    }, 25000);
  }

  function scheduleReconnect() {
    clearReconnectTimer();
    reconnectAttempt += 1;
    const delay = Math.min(1000 * Math.pow(2, reconnectAttempt - 1), 10000);
    setLiveState('connecting', 'Reconnecting live updates…');
    reconnectTimer = window.setTimeout(connectLiveUpdates, delay);
  }

  function connectLiveUpdates() {
    if (!filesCard || !('WebSocket' in window)) {
      return;
    }

    clearReconnectTimer();

    if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
      return;
    }

    const scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${scheme}//${window.location.host}/ws/files`;

    try {
      socket = new WebSocket(wsUrl);
    } catch (_) {
      scheduleReconnect();
      return;
    }

    setLiveState('connecting', 'Connecting live updates…');

    socket.addEventListener('open', () => {
      reconnectAttempt = 0;
      startPingTimer();
      setLiveState('connected', 'Live updates connected');
    });

    socket.addEventListener('message', event => {
      try {
        const payload = JSON.parse(event.data);
        if (!payload || typeof payload !== 'object') {
          return;
        }

        if (payload.event === 'ready' && payload.signature) {
          liveSignature = payload.signature;
          filesCard.dataset.filesSignature = payload.signature;
          return;
        }

        if (payload.event === 'network_state') {
          applyNetworkState(payload, { notify: false });
          return;
        }

        if (payload.event === 'network_changed') {
          applyNetworkState(payload, { notify: true });
          if (isP2PMode) {
            void fetchPeers();
            void fetchTransferSessions();
          }
          return;
        }

        if (payload.event === 'files_changed' && payload.signature && payload.signature !== liveSignature) {
          reloadOrQueue(payload.signature);
        }
      } catch (_) {
        // Ignore malformed frames.
      }
    });

    socket.addEventListener('close', () => {
      clearPingTimer();
      setLiveState('offline', 'Live updates disconnected');
      socket = null;
      scheduleReconnect();
    });

    socket.addEventListener('error', () => {
      clearPingTimer();
      setLiveState('offline', 'Live updates unavailable');
      socket?.close();
    });
  }

  function buildPeerKey(host, transferPort) {
    return `${host}:${transferPort}`;
  }

  function setPeerStatus(text, state = 'idle') {
    if (peerStatusLabel) {
      peerStatusLabel.textContent = text;
      peerStatusLabel.dataset.state = state;
    }
  }

  function getSelectedPeer() {
    if (!peerSelect || !peerSelect.options.length) {
      return null;
    }
    const selectedOption = peerSelect.options[peerSelect.selectedIndex];
    if (!selectedOption) {
      return null;
    }
    const host = selectedOption.dataset.host || '';
    const transferPort = Number(selectedOption.dataset.transferPort || '');
    if (!host || !Number.isFinite(transferPort) || transferPort <= 0) {
      return null;
    }
    return {
      host,
      transferPort,
      deviceName: selectedOption.dataset.deviceName || host
    };
  }

  function updatePeerDetail() {
    if (!peerDetail) {
      return;
    }
    const peer = getSelectedPeer();
    if (!peer) {
      peerDetail.textContent = 'Select a peer, then click Send on any file row.';
      return;
    }
    peerDetail.textContent = `Ready to send files to ${peer.deviceName} (${peer.host}:${peer.transferPort}).`;
  }

  function renderPeerOptions(peers) {
    if (!peerSelect) {
      return;
    }
    const selectedBefore = getSelectedPeer();
    const selectedKey = selectedBefore ? buildPeerKey(selectedBefore.host, selectedBefore.transferPort) : '';
    peerSelect.innerHTML = '';

    if (!peers.length) {
      const option = document.createElement('option');
      option.value = '';
      option.textContent = 'No peers discovered yet';
      peerSelect.appendChild(option);
      peerSelect.disabled = true;
      setPeerStatus('No peers', 'idle');
      updatePeerDetail();
      return;
    }

    peers.forEach(peer => {
      const host = typeof peer.host === 'string' ? peer.host : '';
      const transferPort = Number(peer.transfer_port || peer.port);
      if (!host || !Number.isFinite(transferPort) || transferPort <= 0) {
        return;
      }
      const option = document.createElement('option');
      option.value = buildPeerKey(host, transferPort);
      option.dataset.host = host;
      option.dataset.transferPort = String(transferPort);
      option.dataset.deviceName = String(peer.device_name || peer.app_name || host);
      option.textContent = `${option.dataset.deviceName} - ${host}:${transferPort}`;
      peerSelect.appendChild(option);
    });

    if (!peerSelect.options.length) {
      const option = document.createElement('option');
      option.value = '';
      option.textContent = 'No valid peer entries';
      peerSelect.appendChild(option);
      peerSelect.disabled = true;
      setPeerStatus('No peers', 'idle');
      updatePeerDetail();
      return;
    }

    peerSelect.disabled = false;
    const hasPrevious = Array.from(peerSelect.options).some(option => option.value === selectedKey);
    if (hasPrevious) {
      peerSelect.value = selectedKey;
    }
    setPeerStatus(`${peerSelect.options.length} peer(s)`, 'ready');
    updatePeerDetail();
  }

  async function fetchPeers() {
    if (!isP2PMode || !peerSelect) {
      return;
    }

    try {
      const response = await fetch('/api/peers', {
        method: 'GET',
        headers: {
          'Accept': 'application/json'
        },
        credentials: 'same-origin'
      });
      const payload = await response.json();
      if (!response.ok) {
        throw new Error(payload.error || 'Could not load peers.');
      }
      const peers = Array.isArray(payload.peers) ? payload.peers : [];
      renderPeerOptions(peers);
    } catch (error) {
      setPeerStatus('Unavailable', 'error');
      if (peerDetail) {
        peerDetail.textContent = error.message || 'Peer discovery is currently unavailable.';
      }
    }
  }

  function startPeerPolling() {
    if (!isP2PMode || !peerSelect) {
      return;
    }
    clearPeerPollTimer();
    void fetchPeers();
    peerPollTimer = window.setInterval(() => {
      void fetchPeers();
    }, 6000);
  }

  function renderTransferSessions(sessions) {
    if (!peerSessionList) {
      return;
    }
    const selected = sessions.slice(0, 6);
    if (!selected.length) {
      peerSessionList.innerHTML = '<p class="muted small">No active transfers.</p>';
      return;
    }

    peerSessionList.innerHTML = '';
    selected.forEach(session => {
      const item = document.createElement('div');
      item.className = 'peer-session-item';

      const title = document.createElement('div');
      title.className = 'peer-session-title';
      title.textContent = `${session.direction === 'receive' ? 'Receiving' : 'Sending'} ${session.file_name || 'file'}`;

      const meta = document.createElement('div');
      meta.className = 'peer-session-meta';
      const host = session.peer_host || 'peer';
      const status = session.status || 'unknown';
      const progress = Number(session.progress_percent || 0);
      if (status === 'sending' || status === 'receiving') {
        meta.textContent = `${host} • ${progress}%`;
      } else if (status === 'failed') {
        meta.textContent = `${host} • Failed`;
      } else if (status === 'canceled') {
        meta.textContent = `${host} • Canceled`;
      } else if (status === 'completed') {
        meta.textContent = `${host} • Completed`;
      } else {
        meta.textContent = `${host} • ${status}`;
      }

      item.appendChild(title);
      item.appendChild(meta);
      const canCancel =
        session.direction === 'send' &&
        ['queued', 'connecting', 'sending', 'persisting'].includes(status) &&
        Boolean(session.session_id);
      if (canCancel) {
        const cancelButton = document.createElement('button');
        cancelButton.type = 'button';
        cancelButton.className = 'peer-session-cancel';
        cancelButton.dataset.sessionId = String(session.session_id);
        cancelButton.textContent = 'Cancel';
        item.appendChild(cancelButton);
      }
      peerSessionList.appendChild(item);
    });
  }

  function getInlineProgressNodes(fileId) {
    if (!fileId) {
      return [];
    }
    return Array.from(document.querySelectorAll('.p2p-inline-progress')).filter(node => node.dataset.p2pProgressFor === fileId);
  }

  function setInlineProgress(fileId, { percent = 0, text = '', state = 'sending', visible = true } = {}) {
    const nodes = getInlineProgressNodes(fileId);
    if (!nodes.length) {
      return;
    }
    const boundedPercent = Math.max(0, Math.min(100, Number(percent) || 0));
    nodes.forEach(node => {
      if (visible) {
        node.classList.remove('hidden');
      } else {
        node.classList.add('hidden');
      }
      node.dataset.state = state;
      const fill = node.querySelector('.p2p-inline-fill');
      if (fill) {
        fill.style.width = `${boundedPercent}%`;
      }
      const textNode = node.querySelector('.p2p-inline-text');
      if (textNode) {
        textNode.textContent = text || '';
      }
    });
  }

  function clearInlineProgress(fileId, delayMs = 0) {
    const reset = () => {
      setInlineProgress(fileId, { percent: 0, text: '', state: 'idle', visible: false });
    };
    if (delayMs > 0) {
      window.setTimeout(reset, delayMs);
      return;
    }
    reset();
  }

  function resetTrackedButton(sessionId) {
    const tracked = trackedTransfers.get(sessionId);
    if (!tracked) {
      return;
    }
    tracked.button.textContent = tracked.originalLabel;
    tracked.button.disabled = false;
    delete tracked.button.dataset.transferSessionId;
    trackedTransfers.delete(sessionId);
  }

  function applySessionToTrackedButtons(sessions) {
    const sessionMap = new Map();
    sessions.forEach(session => {
      sessionMap.set(session.session_id, session);
    });

    trackedTransfers.forEach((tracked, sessionId) => {
      const session = sessionMap.get(sessionId);
      if (!session) {
        return;
      }

      const status = session.status || 'queued';
      const progress = Number(session.progress_percent || 0);
      if (status === 'sending') {
        tracked.button.textContent = `${progress}%`;
        tracked.button.disabled = true;
        setInlineProgress(tracked.fileId, { percent: progress, text: `${progress}% sending`, state: 'sending', visible: true });
        return;
      }
      if (status === 'queued' || status === 'connecting') {
        tracked.button.textContent = 'Sending...';
        tracked.button.disabled = true;
        const message = status === 'connecting' ? 'Connecting...' : 'Queued...';
        setInlineProgress(tracked.fileId, { percent: Math.max(progress, 4), text: message, state: status, visible: true });
        return;
      }
      if (status === 'persisting') {
        tracked.button.textContent = 'Finalizing...';
        tracked.button.disabled = true;
        setInlineProgress(tracked.fileId, { percent: Math.max(progress, 98), text: 'Finalizing...', state: 'persisting', visible: true });
        return;
      }
      if (status === 'completed') {
        tracked.button.textContent = 'Sent';
        tracked.button.disabled = true;
        setInlineProgress(tracked.fileId, { percent: 100, text: 'Completed', state: 'completed', visible: true });
        if (!tracked.completedNotified) {
          tracked.completedNotified = true;
          showToast(`Sent ${tracked.fileName} to ${tracked.peerName}.`, 'success', 2000);
          window.setTimeout(() => {
            resetTrackedButton(sessionId);
            clearInlineProgress(tracked.fileId, 0);
          }, 900);
        }
        return;
      }
      if (status === 'canceled') {
        tracked.button.textContent = 'Canceled';
        tracked.button.disabled = true;
        setInlineProgress(tracked.fileId, { percent: 100, text: 'Canceled', state: 'canceled', visible: true });
        if (!tracked.canceledNotified) {
          tracked.canceledNotified = true;
          showToast(`Canceled transfer for ${tracked.fileName}.`, 'error', 2200);
        }
        window.setTimeout(() => {
          resetTrackedButton(sessionId);
          clearInlineProgress(tracked.fileId, 0);
        }, 900);
        return;
      }
      if (status === 'failed') {
        const reason = session.error || 'Transfer failed.';
        setInlineProgress(tracked.fileId, { percent: 100, text: 'Failed', state: 'failed', visible: true });
        if (!tracked.failedNotified) {
          tracked.failedNotified = true;
          showToast(reason, 'error', 3200);
          if (copyFeedback) {
            copyFeedback.textContent = reason;
          }
        }
        resetTrackedButton(sessionId);
        clearInlineProgress(tracked.fileId, 2600);
      }
    });
  }

  async function fetchTransferSessions() {
    if (!isP2PMode) {
      return;
    }
    try {
      const response = await fetch('/api/p2p/sessions', {
        method: 'GET',
        headers: {
          'Accept': 'application/json'
        },
        credentials: 'same-origin'
      });
      const payload = await response.json();
      if (!response.ok) {
        throw new Error(payload.error || 'Could not load transfer sessions.');
      }
      const sessions = Array.isArray(payload.sessions) ? payload.sessions : [];
      renderTransferSessions(sessions);
      applySessionToTrackedButtons(sessions);
    } catch (_) {
      // Keep UI stable if session polling is temporarily unavailable.
    }
  }

  async function handleTransferCancel(event) {
    const button = event.target.closest('.peer-session-cancel');
    if (!button || !filesCard) {
      return;
    }
    const sessionId = button.dataset.sessionId || '';
    const csrfToken = filesCard.dataset.csrfToken;
    if (!sessionId || !csrfToken) {
      return;
    }

    button.disabled = true;
    const originalLabel = button.textContent;
    button.textContent = 'Canceling...';
    try {
      const response = await fetch(`/api/p2p/cancel/${sessionId}`, {
        method: 'POST',
        headers: {
          'X-CSRF-Token': csrfToken,
          'Accept': 'application/json'
        },
        credentials: 'same-origin'
      });
      const payload = await response.json();
      if (!response.ok) {
        throw new Error(payload.error || 'Could not cancel transfer.');
      }
      showToast('Cancel request sent.', 'success', 1600);
      void fetchTransferSessions();
    } catch (error) {
      const errorMessage = error.message || 'Could not cancel transfer.';
      showToast(errorMessage, 'error', 2800);
      if (copyFeedback) {
        copyFeedback.textContent = errorMessage;
      }
      button.textContent = originalLabel;
      button.disabled = false;
    }
  }

  function startTransferPolling() {
    if (!isP2PMode) {
      return;
    }
    clearTransferPollTimer();
    void fetchTransferSessions();
    transferPollTimer = window.setInterval(() => {
      void fetchTransferSessions();
    }, 1400);
  }

  if (input) {
    input.addEventListener('change', () => {
      updateSelectionUi();
      if (!input.files?.length) {
        maybeRefreshQueued();
      }
    });
    updateSelectionUi();
  }

  if (clearSelectionButton && input) {
    clearSelectionButton.addEventListener('click', () => {
      input.value = '';
      updateSelectionUi();
      maybeRefreshQueued();
    });
  }

  if (uploadForm) {
    uploadForm.addEventListener('submit', () => {
      uploadPending = true;
      if (selectedFiles) {
        selectedFiles.textContent = 'Uploading…';
      }
      if (clearSelectionButton) {
        clearSelectionButton.classList.add('hidden');
      }
      if (uploadButton) {
        uploadButton.disabled = true;
        uploadButton.textContent = 'Uploading…';
      }
    });
  }

  if (dropzone && input) {
    ['dragenter', 'dragover'].forEach(eventName => {
      dropzone.addEventListener(eventName, event => {
        event.preventDefault();
        dropzone.classList.add('dragover');
      });
    });

    ['dragleave', 'drop'].forEach(eventName => {
      dropzone.addEventListener(eventName, event => {
        event.preventDefault();
        dropzone.classList.remove('dragover');
      });
    });

    dropzone.addEventListener('drop', event => {
      if (event.dataTransfer?.files?.length) {
        input.files = event.dataTransfer.files;
        updateSelectionUi();
      }
    });
  }

  async function copyToClipboard(text) {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }

    const temp = document.createElement('textarea');
    temp.value = text;
    document.body.appendChild(temp);
    temp.select();
    document.execCommand('copy');
    temp.remove();
    return true;
  }

  async function handleShare(event) {
    const button = event.target.closest('.share-button');
    if (!button || !filesCard) {
      return;
    }

    const fileId = button.dataset.fileId;
    const csrfToken = filesCard.dataset.csrfToken;
    if (!fileId || !csrfToken) {
      return;
    }

    button.disabled = true;
    const originalLabel = button.textContent;
    button.textContent = 'Creating…';

    try {
      const response = await fetch(`/api/files/${fileId}/share`, {
        method: 'POST',
        headers: {
          'X-CSRF-Token': csrfToken,
          'Accept': 'application/json'
        },
        credentials: 'same-origin'
      });

      const payload = await response.json();
      if (!response.ok) {
        throw new Error(payload.error || 'Could not create share link.');
      }

      await copyToClipboard(payload.url);
      const successMessage = `Copied a ${Math.round(payload.expires_in_seconds / 60)} minute link for ${payload.filename}.`;
      if (copyFeedback) {
        copyFeedback.textContent = successMessage;
      }
      showToast(successMessage, 'success', 2200);
      button.textContent = 'Copied';
      window.setTimeout(() => {
        button.textContent = originalLabel;
        button.disabled = false;
      }, 1200);
      return;
    } catch (error) {
      const errorMessage = error.message || 'Could not copy the share link.';
      if (copyFeedback) {
        copyFeedback.textContent = errorMessage;
      }
      showToast(errorMessage, 'error', 3000);
      button.textContent = originalLabel;
      button.disabled = false;
    }
  }

  async function handleP2PSend(event) {
    const button = event.target.closest('.p2p-send-button');
    if (!button || !filesCard) {
      return;
    }

    const fileId = button.dataset.fileId;
    const fileName = button.dataset.filename || 'file';
    const csrfToken = filesCard.dataset.csrfToken;
    if (!fileId || !csrfToken) {
      return;
    }

    const peer = getSelectedPeer();
    if (!peer) {
      const message = 'Select a target peer before sending files.';
      if (copyFeedback) {
        copyFeedback.textContent = message;
      }
      showToast(message, 'error', 3000);
      return;
    }

    button.disabled = true;
    const originalLabel = button.textContent;
    button.textContent = 'Queued...';
    setInlineProgress(fileId, { percent: 3, text: 'Queued...', state: 'queued', visible: true });

    try {
      const response = await fetch(`/api/p2p/send/${fileId}`, {
        method: 'POST',
        headers: {
          'X-CSRF-Token': csrfToken,
          'Accept': 'application/json',
          'Content-Type': 'application/json'
        },
        credentials: 'same-origin',
        body: JSON.stringify({
          host: peer.host,
          transfer_port: peer.transferPort
        })
      });

      const payload = await response.json();
      if (!response.ok) {
        throw new Error(payload.error || 'Could not send file to peer.');
      }
      const sessionId = String(payload.session_id || '');
      if (!sessionId) {
        throw new Error('Transfer session id missing from server response.');
      }
      trackedTransfers.set(sessionId, {
        button,
        originalLabel,
        fileId,
        fileName,
        peerName: peer.deviceName,
        completedNotified: false,
        failedNotified: false,
        canceledNotified: false
      });
      button.dataset.transferSessionId = sessionId;
      button.textContent = 'Sending...';
      setInlineProgress(fileId, { percent: 6, text: 'Connecting...', state: 'connecting', visible: true });
      const successMessage = `Queued ${fileName} for ${peer.deviceName}.`;
      if (copyFeedback) {
        copyFeedback.textContent = successMessage;
      }
      showToast(successMessage, 'success', 1800);
      void fetchTransferSessions();
      return;
    } catch (error) {
      const errorMessage = error.message || 'Could not send file to peer.';
      if (copyFeedback) {
        copyFeedback.textContent = errorMessage;
      }
      showToast(errorMessage, 'error', 3200);
      setInlineProgress(fileId, { percent: 100, text: 'Failed', state: 'failed', visible: true });
      clearInlineProgress(fileId, 2400);
      button.textContent = originalLabel;
      button.disabled = false;
    }
  }

  hydrateInitialToast();
  initSidebarToggle();
  renderBulkSelectionState();

  if (selectAllFilesCheckbox) {
    selectAllFilesCheckbox.addEventListener('change', handleSelectAllChange);
  }

  if (bulkDownloadButton) {
    bulkDownloadButton.addEventListener('click', () => {
      void handleBatchDownload();
    });
  }

  if (isP2PMode && peerSelect) {
    peerSelect.addEventListener('change', updatePeerDetail);
    updatePeerDetail();
  }
  if (isP2PMode && peerSessionList) {
    peerSessionList.addEventListener('click', event => {
      void handleTransferCancel(event);
    });
  }

  if (filesCard) {
    connectLiveUpdates();
    startPeerPolling();
    startTransferPolling();
    filesCard.addEventListener('change', event => {
      handleFileSelectionChange(event);
    });
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible' && (!socket || socket.readyState === WebSocket.CLOSED)) {
        connectLiveUpdates();
      }
      maybeRefreshQueued();
      if (document.visibilityState === 'visible') {
        void fetchPeers();
        void fetchTransferSessions();
      }
    });
    window.addEventListener('focus', () => {
      maybeRefreshQueued();
      if (!socket || socket.readyState === WebSocket.CLOSED) {
        connectLiveUpdates();
      }
      void fetchPeers();
      void fetchTransferSessions();
    });
    window.addEventListener('beforeunload', () => {
      clearPingTimer();
      clearReconnectTimer();
      clearPeerPollTimer();
      clearTransferPollTimer();
      if (socket && socket.readyState === WebSocket.OPEN) {
        socket.close(1000, 'Page unload');
      }
    });
    filesCard.addEventListener('click', event => {
      void handleShare(event);
      void handleP2PSend(event);
    });
  }

  document.querySelectorAll('.delete-form').forEach(form => {
    form.addEventListener('submit', event => {
      const fileName = form.dataset.fileName || 'this file';
      if (!window.confirm(`Delete ${fileName}?`)) {
        event.preventDefault();
      }
    });
  });

  document.querySelectorAll('.logout-confirm-form').forEach(form => {
    form.addEventListener('submit', event => {
      if (!window.confirm('Are you sure you want to log out?')) {
        event.preventDefault();
      }
    });
  });
})();
