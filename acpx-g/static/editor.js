// ─── acpx-g Workflow Editor ───────────────────────────────────────
// Vanilla JS + Drawflow + js-yaml + Dagre + CodeMirror 5

'use strict';

// ── State ──────────────────────────────────────────────────────────
let dfEditor = null;          // Drawflow instance
let cmEditor = null;          // CodeMirror instance
let selectedNodeId = null;    // Currently selected Drawflow node ID
let nodeIdCounter = 0;        // Auto-increment counter for node IDs
let yamlDirty = false;        // Whether YAML needs re-sync
let historyStack = [];        // Undo stack (snapshots)
let redoStack = [];           // Redo stack
const MAX_HISTORY = 50;
let debounceTimer = null;

// Map: drawflow node ID → business node ID (e.g. "1" → "build")
const dfIdToBizId = new Map();
const bizIdToDfId = new Map();

// Workflow metadata (separate from nodes)
let wfMeta = {
  name: 'new-workflow',
  version: '1.0',
  description: '',
  timeout: null,
  defaults: { retry: 0, timeout: 300, shell: 'bash -c' },
  inputs: {},
  env: {},
  references: {},
};

// Base directory for resolving relative references (from template file path)
let wfBaseDir = null;

// Node data store: bizId → { type, ...fields }
const nodeStore = new Map();

// ── Tab Switching ──────────────────────────────────────────────────
function switchTab(tab) {
  document.querySelectorAll('#tab-bar .tab').forEach(t =>
    t.classList.toggle('active', t.dataset.tab === tab));
  document.getElementById('logs-view').style.display = tab === 'logs' ? 'flex' : 'none';
  document.getElementById('editor-view').style.display = tab === 'editor' ? 'flex' : 'none';
  if (tab === 'editor') {
    if (!dfEditor) initEditor();
    refreshEditorTemplateList();
  }
}

// Called from Dashboard "Edit" button — switches tab and loads template synchronously
function editTemplateInEditor(name) {
  switchTab('editor');
  loadTemplateToEditor(name);
}

// ── Editor Initialization ──────────────────────────────────────────
function initEditor() {
  if (typeof Drawflow === 'undefined') {
    showToast('Editor libraries still loading, please try again in a moment', 'error');
    return;
  }
  const el = document.getElementById('drawflow');
  dfEditor = new Drawflow(el);
  dfEditor.reroute = true;
  dfEditor.reroute_fix_curvature = true;
  dfEditor.force_first_input = false;
  dfEditor.start();

  // Event listeners
  dfEditor.on('nodeCreated', function(id) {
    updateYamlFromCanvas();
    pushHistory();
  });
  dfEditor.on('nodeRemoved', function(id) {
    const bizId = dfIdToBizId.get(String(id));
    if (bizId) {
      nodeStore.delete(bizId);
      dfIdToBizId.delete(String(id));
      bizIdToDfId.delete(bizId);
    }
    if (selectedNodeId === id) hidePropertyPanel();
    updateYamlFromCanvas();
    pushHistory();
  });
  dfEditor.on('connectionCreated', function(conn) {
    // Update depends in nodeStore
    const fromBizId = dfIdToBizId.get(String(conn.output_id));
    const toBizId = dfIdToBizId.get(String(conn.input_id));
    if (fromBizId && toBizId) {
      const nd = nodeStore.get(toBizId);
      if (nd && !nd.depends.includes(fromBizId)) {
        nd.depends.push(fromBizId);
      }
    }
    updateYamlFromCanvas();
    pushHistory();
  });
  dfEditor.on('connectionRemoved', function(conn) {
    const fromBizId = dfIdToBizId.get(String(conn.output_id));
    const toBizId = dfIdToBizId.get(String(conn.input_id));
    if (fromBizId && toBizId) {
      const nd = nodeStore.get(toBizId);
      if (nd) nd.depends = nd.depends.filter(d => d !== fromBizId);
    }
    updateYamlFromCanvas();
    pushHistory();
  });
  dfEditor.on('nodeSelected', function(id) {
    selectedNodeId = id;
    showPropertyPanel(id);
  });
  dfEditor.on('nodeUnselected', function() {
    selectedNodeId = null;
    hidePropertyPanel();
  });
  dfEditor.on('nodeMoved', function(id) {
    // Don't push history on every move, just update
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => updateYamlFromCanvas(), 300);
  });
  dfEditor.on('zoom', function(zoom) { /* no-op */ });
  dfEditor.on('translate', function(pos) { /* no-op */ });

  // Keyboard shortcuts
  document.addEventListener('keydown', function(e) {
    if (document.getElementById('editor-view').style.display === 'none') return;
    if (e.ctrlKey || e.metaKey) {
      if (e.key === 'z' && !e.shiftKey) { e.preventDefault(); editorUndo(); }
      else if (e.key === 'y' || (e.key === 'z' && e.shiftKey)) { e.preventDefault(); editorRedo(); }
      else if (e.key === 's') { e.preventDefault(); editorSave(); }
    }
    if (e.key === 'Delete' || e.key === 'Backspace') {
      // Only delete if not focused on an input/textarea
      const tag = document.activeElement?.tagName;
      if (tag !== 'INPUT' && tag !== 'TEXTAREA' && !document.activeElement?.classList.contains('CodeMirror-code')) {
        editorDeleteSelected();
      }
    }
  });

  // Palette drag events
  document.querySelectorAll('.palette-node').forEach(el => {
    el.addEventListener('dragstart', function(e) {
      e.dataTransfer.setData('node-type', el.dataset.type);
    });
  });

  // Load auto-saved draft if exists
  loadDraft();
}

// ── Drop Handler ───────────────────────────────────────────────────
function editorDrop(e) {
  e.preventDefault();
  const type = e.dataTransfer.getData('node-type');
  if (!type) return;

  const bizId = generateBizId(type);
  const data = defaultNodeData(type, bizId);
  nodeStore.set(bizId, data);

  const html = buildNodeHtml(bizId, type, data);
  let posX, posY;
  if (dfEditor.precanvas) {
    const pos = calculateCanvasPos(e);
    posX = pos.x;
    posY = pos.y;
  } else {
    posX = e.clientX;
    posY = e.clientY;
  }
  const dfId = dfEditor.addNode(bizId, 1, 1, posX, posY, type, { bizId, type }, html);

  dfIdToBizId.set(String(dfId), bizId);
  bizIdToDfId.set(bizId, dfId);

  updateYamlFromCanvas();
  pushHistory();
  saveDraft();
}

function calculateCanvasPos(e) {
  const canvas = document.getElementById('drawflow');
  const rect = canvas.getBoundingClientRect();
  return {
    x: (e.clientX - rect.left) / (dfEditor.zoom || 1) - (dfEditor.precanvas?.scrollLeft || 0),
    y: (e.clientY - rect.top) / (dfEditor.zoom || 1) - (dfEditor.precanvas?.scrollTop || 0)
  };
}

// ── Node ID Generation ────────────────────────────────────────────
function generateBizId(type) {
  nodeIdCounter++;
  const prefix = { shell: 'step', agent: 'agent', reference: 'ref' }[type] || 'node';
  return `${prefix}-${nodeIdCounter}`;
}

function sanitizeBizId(s) {
  return s.replace(/[^a-zA-Z0-9_\-./]/g, '-');
}

// ── Default Node Data ──────────────────────────────────────────────
function defaultNodeData(type, bizId) {
  const base = { type, depends: [], env: {}, outputs: {}, continue_on_error: false, timeout: null, retry: null, shell: null, if_condition: null };
  switch (type) {
    case 'shell': return { ...base, run: 'echo hello' };
    case 'agent': return { ...base, prompt: 'Review the code', agent: 'peri', model: null, cwd: null };
    case 'reference': return { ...base, ref: '', with: {} };
    default: return base;
  }
}

// ── Build Node HTML ────────────────────────────────────────────────
function buildNodeHtml(bizId, type, data) {
  const icons = { shell: '&#9654;', agent: '&#10023;', reference: '&#8635;' };
  const labels = { shell: 'Shell', agent: 'Agent', reference: 'Ref' };
  const preview = getNodePreview(type, data);
  const depsHtml = (data.depends && data.depends.length)
    ? '<div class="enode-deps">' + data.depends.map(d => `<span class="enode-dep-tag">${esc(d)}</span>`).join('') + '</div>'
    : '';
  return `<div class="enode ${type}-node">
    <div class="enode-header">
      <span class="enode-icon">${icons[type]}</span>
      <span class="enode-type">${labels[type]}</span>
      <span class="enode-id">${esc(bizId)}</span>
    </div>
    <div class="enode-body">
      <div class="enode-preview">${esc(preview)}</div>
      ${depsHtml}
    </div>
  </div>`;
}

function getNodePreview(type, data) {
  switch (type) {
    case 'shell': {
      const run = data.run || '';
      return run.split('\n')[0].substring(0, 60);
    }
    case 'agent': return (data.prompt || '').split('\n')[0].substring(0, 60);
    case 'reference': return data.ref ? `ref: ${data.ref}` : '(no ref)';
    default: return '';
  }
}

function refreshNodeHtml(dfId) {
  const bizId = dfIdToBizId.get(String(dfId));
  if (!bizId) return;
  const data = nodeStore.get(bizId);
  if (!data) return;
  const html = buildNodeHtml(bizId, data.type, data);
  // Drawflow doesn't have a clean API to update HTML, so we manipulate DOM directly
  const nodeEl = document.querySelector(`#drawflow .drawflow-node[data-id="${dfId}"] .drawflow_content_node`);
  if (nodeEl) nodeEl.innerHTML = html;
}

// ── Property Panel ─────────────────────────────────────────────────
function showPropertyPanel(dfId) {
  const bizId = dfIdToBizId.get(String(dfId));
  if (!bizId) return;
  const data = nodeStore.get(bizId);
  if (!data) return;

  document.getElementById('prop-empty').style.display = 'none';
  const content = document.getElementById('prop-content');
  content.style.display = 'block';

  let html = '';

  // Identity
  html += `<div class="prop-section">
    <div class="prop-section-title">Identity</div>
    <div class="prop-field">
      <label>Node ID</label>
      <input type="text" value="${esc(bizId)}" data-df-id="${dfId}" data-action="rename">
    </div>
    <div class="prop-field">
      <label>Type</label>
      <select disabled><option>${esc(data.type)}</option></select>
    </div>
  </div>`;

  // Type-specific fields
  if (data.type === 'shell') {
    html += propSection('Command',
      propFieldTextarea('Run', 'run', data.run || '', dfId)
    );
  } else if (data.type === 'agent') {
    html += propSection('Agent Config',
      propFieldTextarea('Prompt', 'prompt', data.prompt || '', dfId) +
      propFieldInput('Agent CLI', 'agent', data.agent || 'peri', dfId, 'text') +
      propFieldSelect('Model', 'model', data.model || '', ['', 'opus', 'sonnet', 'haiku'], dfId) +
      propFieldInput('Working Dir', 'cwd', data.cwd || '', dfId, 'text')
    );
  } else if (data.type === 'reference') {
    const refKeys = Object.keys(wfMeta.references);
    html += propSection('Reference',
      (refKeys.length
        ? propFieldSelect('Reference', 'ref', data.ref || '', refKeys, dfId)
        : propFieldInput('Reference Key', 'ref', data.ref || '', dfId, 'text')) +
      buildKVEditor('Parameters (with)', 'with', data.with || {}, dfId)
    );
  }

  // Execution config
  html += propSection('Execution',
    propFieldInput('Timeout (s)', 'timeout', data.timeout || '', dfId, 'number') +
    propFieldInput('Retry', 'retry', data.retry != null ? data.retry : '', dfId, 'number') +
    propFieldInput('Shell', 'shell', data.shell || '', dfId, 'text') +
    propFieldInput('If condition', 'if_condition', data.if_condition || '', dfId, 'text') +
    `<div class="prop-field">
      <label><input type="checkbox" ${data.continue_on_error ? 'checked' : ''} data-df-id="${dfId}" data-key="continue_on_error" data-action="checkbox"> <span class="checkbox-label">Continue on error</span></label>
    </div>`
  );

  // Dependencies (read-only display)
  html += propSection('Dependencies', `
    <div class="prop-field">
      <label>Depends on (draw connections to set)</label>
      <div style="font-size:12px;color:var(--text);padding:4px 0">
        ${data.depends.length ? data.depends.map(d => `<span class="enode-dep-tag">${esc(d)}</span>`).join(' ') : '<span style="color:var(--text2)">No dependencies</span>'}
      </div>
    </div>
  `);

  // Environment variables
  html += propSection('Environment', buildKVEditor('Variables', 'env', data.env || {}, dfId));

  // Outputs
  html += propSection('Outputs', buildKVEditor('Output mappings', 'outputs', data.outputs || {}, dfId));

  // Delete button
  html += `<div class="prop-section" style="text-align:right">
    <button class="btn btn-sm btn-danger" data-df-id="${dfId}" data-action="delete-node">Delete Node</button>
  </div>`;

  content.innerHTML = html;
  bindPropertyEvents(content);
}

// Bind all property panel events via delegation (avoids inline handler escaping issues)
function bindPropertyEvents(container) {
  // Text/number/select inputs with data-key
  container.querySelectorAll('input[data-key][data-df-id], select[data-key][data-df-id], textarea[data-key][data-df-id]').forEach(el => {
    el.addEventListener('change', function() {
      const dfId = Number(this.dataset.dfId);
      const key = this.dataset.key;
      let val = this.value;
      if (this.type === 'number') val = val ? Number(val) : null;
      if (this.type === 'checkbox') val = this.checked;
      updateNodeData(dfId, key, val);
    });
  });
  // Rename input
  container.querySelectorAll('input[data-action="rename"]').forEach(el => {
    el.addEventListener('change', function() {
      renameNode(Number(this.dataset.dfId), this.value);
    });
  });
  // Checkbox
  container.querySelectorAll('input[data-action="checkbox"]').forEach(el => {
    el.addEventListener('change', function() {
      const dfId = Number(this.dataset.dfId);
      const key = this.dataset.key;
      updateNodeData(dfId, key, this.checked);
    });
  });
  // Delete node
  container.querySelectorAll('button[data-action="delete-node"]').forEach(el => {
    el.addEventListener('click', function() {
      dfEditor.removeNodeId('node-' + this.dataset.dfId);
    });
  });
  // KV editors
  container.querySelectorAll('.kv-add').forEach(btn => {
    btn.addEventListener('click', function() {
      addKV(this, Number(this.dataset.dfId), this.dataset.field);
    });
  });
  container.querySelectorAll('.kv-remove').forEach(btn => {
    btn.addEventListener('click', function() {
      removeKV(this, Number(this.dataset.dfId), this.dataset.field);
    });
  });
  container.querySelectorAll('.kv-row input').forEach(inp => {
    inp.addEventListener('change', function() {
      const list = this.closest('.kv-list');
      if (list) updateKVFromList(list);
    });
  });
}

function hidePropertyPanel() {
  document.getElementById('prop-empty').style.display = 'block';
  document.getElementById('prop-content').style.display = 'none';
}

// ── Property helpers (data-attr based, no inline handlers) ──────────
function propSection(title, body) {
  return `<div class="prop-section"><div class="prop-section-title">${title}</div>${body}</div>`;
}
function propFieldInput(label, key, val, dfId, type) {
  return `<div class="prop-field"><label>${label}</label><input type="${type || 'text'}" value="${esc(String(val ?? ''))}" data-df-id="${dfId}" data-key="${key}"></div>`;
}
function propFieldSelect(label, key, val, options, dfId) {
  const opts = options.map(o => `<option value="${esc(o)}" ${o === val ? 'selected' : ''}>${esc(o || '(default)')}</option>`).join('');
  return `<div class="prop-field"><label>${label}</label><select data-df-id="${dfId}" data-key="${key}">${opts}</select></div>`;
}
function propFieldTextarea(label, key, val, dfId) {
  return `<div class="prop-field"><label>${label}</label><textarea data-df-id="${dfId}" data-key="${key}">${esc(val || '')}</textarea></div>`;
}

function buildKVEditor(label, field, obj, dfId) {
  const entries = Object.entries(obj || {});
  let html = `<div class="prop-field"><label>${label}</label><div class="kv-list" data-field="${field}" data-df-id="${dfId}">`;
  entries.forEach(([k, v]) => {
    html += `<div class="kv-row">
      <input value="${esc(k)}" placeholder="key">
      <input value="${esc(v)}" placeholder="value">
      <button class="kv-remove" data-df-id="${dfId}" data-field="${field}">&times;</button>
    </div>`;
  });
  html += `</div><button class="kv-add" data-df-id="${dfId}" data-field="${field}">+ Add</button></div>`;
  return html;
}

// ── Data Update Handlers ───────────────────────────────────────────
function updateNodeData(dfId, key, value) {
  const bizId = dfIdToBizId.get(String(dfId));
  if (!bizId) return;
  const data = nodeStore.get(bizId);
  if (!data) return;
  data[key] = value;
  refreshNodeHtml(dfId);
  updateYamlFromCanvas();
  pushHistory();
  saveDraft();
}

function renameNode(dfId, newId) {
  const newBizId = sanitizeBizId(newId);
  if (!newBizId) return;
  const oldBizId = dfIdToBizId.get(String(dfId));
  if (!oldBizId || oldBizId === newBizId) return;

  // Check uniqueness
  if (nodeStore.has(newBizId) && newBizId !== oldBizId) {
    showToast('Node ID already exists', 'error');
    return;
  }

  // Update all references
  const data = nodeStore.get(oldBizId);
  nodeStore.delete(oldBizId);
  nodeStore.set(newBizId, data);

  dfIdToBizId.set(String(dfId), newBizId);
  bizIdToDfId.delete(oldBizId);
  bizIdToDfId.set(newBizId, dfId);

  // Update depends in other nodes
  nodeStore.forEach((nd) => {
    nd.depends = nd.depends.map(d => d === oldBizId ? newBizId : d);
  });

  refreshNodeHtml(dfId);
  updateYamlFromCanvas();
  pushHistory();
  saveDraft();
  showPropertyPanel(dfId);
}

function addKV(btn, dfId, field) {
  const bizId = dfIdToBizId.get(String(dfId));
  if (!bizId) return;
  const data = nodeStore.get(bizId);
  if (!data) return;
  const obj = data[field] || {};
  const newKey = 'key_' + Object.keys(obj).length;
  obj[newKey] = '';
  data[field] = obj;
  showPropertyPanel(dfId);
  updateYamlFromCanvas();
  pushHistory();
  saveDraft();
}

function removeKV(btn, dfId, field) {
  const bizId = dfIdToBizId.get(String(dfId));
  if (!bizId) return;
  const data = nodeStore.get(bizId);
  if (!data) return;
  const row = btn.closest('.kv-row');
  row.remove();
  // Rebuild from DOM
  updateKVFromList(btn.closest('.kv-list'));
  pushHistory();
  saveDraft();
}

// Rebuild KV data from a kv-list DOM element
function updateKVFromList(listEl) {
  if (!listEl) return;
  const dfId = listEl.dataset.dfId;
  const field = listEl.dataset.field;
  const bizId = dfIdToBizId.get(String(dfId));
  if (!bizId) return;
  const data = nodeStore.get(bizId);
  if (!data) return;
  const obj = {};
  listEl.querySelectorAll('.kv-row').forEach(r => {
    const inputs = r.querySelectorAll('input');
    if (inputs[0] && inputs[0].value) obj[inputs[0].value] = inputs[1] ? inputs[1].value : '';
  });
  data[field] = obj;
  updateYamlFromCanvas();
  saveDraft();
}

// ── YAML Sync: Canvas → YAML ──────────────────────────────────────
function updateYamlFromCanvas() {
  const yaml = exportToYaml();
  if (cmEditor && document.getElementById('yaml-editor-wrap').style.display !== 'none') {
    const cursor = cmEditor.getCursor();
    cmEditor.setValue(yaml);
    cmEditor.setCursor(cursor);
  }
  yamlDirty = true;
}

function exportToYaml() {
  const nodes = [];
  nodeStore.forEach((data, bizId) => {
    const node = { id: bizId, type: data.type };
    if (data.depends && data.depends.length) node.depends = [...data.depends];

    if (data.type === 'shell') {
      node.run = data.run || 'echo hello';
    } else if (data.type === 'agent') {
      node.prompt = data.prompt || '';
      if (data.agent && data.agent !== 'peri') node.agent = data.agent;
      if (data.model) node.model = data.model;
      if (data.cwd) node.cwd = data.cwd;
    } else if (data.type === 'reference') {
      node.ref = data.ref || '';
      if (data.with && Object.keys(data.with).length) node.with = { ...data.with };
    }

    // Common fields (only include non-default)
    if (data.timeout != null) node.timeout = data.timeout;
    if (data.retry != null) node.retry = data.retry;
    if (data.shell) node.shell = data.shell;
    if (data.if_condition) node.if = data.if_condition;
    if (data.continue_on_error) node.continue_on_error = true;
    if (data.env && Object.keys(data.env).length) node.env = { ...data.env };
    if (data.outputs && Object.keys(data.outputs).length) node.outputs = { ...data.outputs };

    nodes.push(node);
  });

  const wf = {
    name: wfMeta.name,
    version: wfMeta.version,
  };
  if (wfMeta.description) wf.description = wfMeta.description;
  if (wfMeta.timeout) wf.timeout = wfMeta.timeout;

  // Defaults
  const defs = wfMeta.defaults;
  if (defs.retry !== 0 || defs.timeout !== 300 || defs.shell !== 'bash -c') {
    wf.defaults = {};
    if (defs.retry !== 0) wf.defaults.retry = defs.retry;
    if (defs.timeout !== 300) wf.defaults.timeout = defs.timeout;
    if (defs.shell !== 'bash -c') wf.defaults.shell = defs.shell;
  }

  if (Object.keys(wfMeta.inputs).length) wf.inputs = wfMeta.inputs;
  if (Object.keys(wfMeta.env).length) wf.env = { ...wfMeta.env };
  if (Object.keys(wfMeta.references).length) wf.references = { ...wfMeta.references };

  wf.nodes = nodes;

  return jsyaml.dump(wf, { indent: 2, lineWidth: 120, noRefs: true, sortKeys: false });
}

// ── YAML Sync: YAML → Canvas ──────────────────────────────────────
function importFromYaml(yamlStr) {
  try {
    const parsed = jsyaml.load(yamlStr);
    if (!parsed || typeof parsed !== 'object') throw new Error('Invalid YAML structure');

    // Clear canvas
    dfEditor.clear();
    nodeStore.clear();
    dfIdToBizId.clear();
    bizIdToDfId.clear();
    selectedNodeId = null;
    hidePropertyPanel();

    // Set metadata
    wfMeta.name = parsed.name || 'untitled';
    wfMeta.version = parsed.version || '1.0';
    wfMeta.description = parsed.description || '';
    wfMeta.timeout = parsed.timeout || null;
    wfMeta.defaults = {
      retry: parsed.defaults?.retry ?? 0,
      timeout: parsed.defaults?.timeout ?? 300,
      shell: parsed.defaults?.shell ?? 'bash -c',
    };
    wfMeta.inputs = parsed.inputs || {};
    wfMeta.env = parsed.env || {};
    wfMeta.references = parsed.references || {};

    document.getElementById('wf-name').value = wfMeta.name;
    document.getElementById('wf-version').value = wfMeta.version;

    // Create nodes
    const nodes = parsed.nodes || [];
    const idMap = {}; // bizId → dfId

    nodes.forEach(node => {
      const bizId = node.id;
      const type = node.type || 'shell';
      const data = defaultNodeData(type, bizId);

      // Populate type-specific fields
      if (type === 'shell') {
        data.run = typeof node.run === 'string' ? node.run : 'echo hello';
      } else if (type === 'agent') {
        data.prompt = typeof node.prompt === 'string' ? node.prompt : '';
        data.agent = node.agent || 'peri';
        data.model = node.model || null;
        data.cwd = node.cwd || null;
      } else if (type === 'reference') {
        data.ref = node.ref || '';
        data.with = node.with || {};
      }

      // Common fields
      data.depends = node.depends || [];
      data.timeout = node.timeout ?? null;
      data.retry = node.retry ?? null;
      data.shell = node.shell || null;
      data.if_condition = node.if || null;
      data.continue_on_error = node.continue_on_error || false;
      data.env = node.env || {};
      data.outputs = node.outputs || {};

      nodeStore.set(bizId, data);

      const html = buildNodeHtml(bizId, type, data);
      const dfId = dfEditor.addNode(
        bizId, 1, 1,
        100 + Math.random() * 300,
        100 + Math.random() * 300,
        type, { bizId, type }, html
      );

      dfIdToBizId.set(String(dfId), bizId);
      bizIdToDfId.set(bizId, dfId);
      idMap[bizId] = dfId;
    });

    // Create connections from depends
    nodes.forEach(node => {
      const bizId = node.id;
      const dfId = idMap[bizId];
      if (!dfId) return;
      (node.depends || []).forEach(depBizId => {
        const depDfId = idMap[depBizId];
        if (depDfId != null) {
          dfEditor.addConnection(depDfId, dfId, 'output_1', 'input_1');
        }
      });
    });

    // Auto-layout after import
    setTimeout(() => editorAutoLayout(), 100);

    updateYamlFromCanvas();
    clearHistory();
    pushHistory();
    saveDraft();
    showToast('Workflow imported successfully', 'success');
  } catch (e) {
    showToast('Import failed: ' + e.message, 'error');
  }
}

// ── Apply YAML changes back to canvas ──────────────────────────────
function applyYamlChanges() {
  if (!cmEditor) return;
  const yaml = cmEditor.getValue();
  importFromYaml(yaml);
}

// ── Auto Layout (Dagre) ────────────────────────────────────────────
function editorAutoLayout() {
  if (typeof dagre === 'undefined') return;

  const exportData = dfEditor.export();
  const moduleData = exportData.drawflow?.Home?.data;
  if (!moduleData) return;

  const nodeIds = Object.keys(moduleData);
  if (!nodeIds.length) return;

  const NODE_W = 200, NODE_H = 80;
  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: 'TB', nodesep: 60, ranksep: 100, marginx: 60, marginy: 60 });
  g.setDefaultEdgeLabel(() => ({}));

  // Add nodes to dagre
  nodeIds.forEach(id => {
    g.setNode(id, { width: NODE_W, height: NODE_H });
  });

  // Add edges from depends
  nodeStore.forEach((data, bizId) => {
    const dfId = bizIdToDfId.get(bizId);
    if (!dfId) return;
    (data.depends || []).forEach(depBizId => {
      const depDfId = bizIdToDfId.get(depBizId);
      if (depDfId != null) g.setEdge(String(depDfId), String(dfId));
    });
  });

  dagre.layout(g);

  // Apply positions via DOM + internal data
  const canvasEl = document.getElementById('drawflow');
  const zoom = dfEditor.zoom || 1;

  g.nodes().forEach(id => {
    const pos = g.node(id);
    if (!pos) return;
    const x = pos.x - NODE_W / 2;
    const y = pos.y - NODE_H / 2;

    // Update Drawflow internal data
    if (moduleData[id]) {
      moduleData[id].pos_x = x;
      moduleData[id].pos_y = y;
    }

    // Update DOM position
    const nodeEl = dfEditor.precanvas.querySelector('#node-' + id);
    if (nodeEl) {
      nodeEl.style.left = x + 'px';
      nodeEl.style.top = y + 'px';
    }
  });

  // Refresh connections
  nodeIds.forEach(id => {
    dfEditor.updateConnectionNodes('node-' + id);
  });
}

// ── Validation ─────────────────────────────────────────────────────
function validateWorkflow() {
  const errors = [];
  const yaml = exportToYaml();
  let parsed;
  try {
    parsed = jsyaml.load(yaml);
  } catch (e) {
    errors.push('YAML parse error: ' + e.message);
    displayValidation(errors);
    return errors;
  }

  if (!parsed) { errors.push('Empty workflow'); displayValidation(errors); return errors; }
  if (!parsed.name?.trim()) errors.push('Workflow name is required');
  if (!parsed.version?.trim()) errors.push('Workflow version is required');
  if (!parsed.nodes?.length) errors.push('At least one node is required');

  const ids = new Set();
  (parsed.nodes || []).forEach(node => {
    const id = node.id;
    if (!id) { errors.push('Node missing ID'); return; }
    if (ids.has(id)) errors.push(`Duplicate node ID: ${id}`);
    ids.add(id);
    if (/[^a-zA-Z0-9_\-./]/.test(id)) errors.push(`Invalid characters in node ID: ${id}`);
    if (node.depends) {
      node.depends.forEach(d => {
        if (d === id) errors.push(`Node '${id}' depends on itself`);
        else if (!ids.has(d)) errors.push(`Node '${id}' depends on non-existent '${d}'`);
      });
    }
    if (node.type === 'reference' && node.ref && parsed.references && !parsed.references[node.ref]) {
      errors.push(`Reference node '${id}' uses undefined ref '${node.ref}'`);
    }
  });

  // Cycle detection
  const adj = {};
  (parsed.nodes || []).forEach(n => { adj[n.id] = n.depends || []; });
  const visited = new Set(), stack = new Set();
  function dfs(id) {
    if (stack.has(id)) return true;
    if (visited.has(id)) return false;
    visited.add(id); stack.add(id);
    for (const dep of (adj[id] || [])) { if (dfs(dep)) return true; }
    stack.delete(id);
    return false;
  }
  for (const id of ids) { if (dfs(id)) { errors.push('Cycle detected in dependency graph'); break; } }

  displayValidation(errors);
  return errors;
}

function displayValidation(errors) {
  const statusEl = document.getElementById('validation-status');
  const errorsEl = document.getElementById('validation-errors');
  if (!errors.length) {
    statusEl.className = 'validation-ok';
    statusEl.textContent = 'No errors';
    errorsEl.innerHTML = '';
  } else {
    statusEl.className = 'validation-err';
    statusEl.textContent = `${errors.length} error(s)`;
    errorsEl.innerHTML = errors.map(e => `<div>${esc(e)}</div>`).join('');
  }
}

function editorValidate() {
  const errors = validateWorkflow();
  if (errors.length) {
    showToast(`${errors.length} validation error(s)`, 'error');
  } else {
    // Also validate server-side
    const yaml = exportToYaml();
    fetch('/api/v1/workflows/validate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ yaml }),
    }).then(r => r.json()).then(result => {
      if (result.valid) {
        showToast('Workflow is valid', 'success');
      } else {
        const msgs = (result.errors || []).map(e => e.message).join('; ');
        showToast('Server validation: ' + msgs, 'error');
      }
    }).catch(() => {
      showToast('Workflow structure is valid (server validation skipped)', 'success');
    });
  }
}

// ── Undo / Redo ────────────────────────────────────────────────────
function currentSnapshot() {
  return {
    meta: JSON.parse(JSON.stringify(wfMeta)),
    nodes: JSON.parse(JSON.stringify(Object.fromEntries(nodeStore))),
    dfData: dfEditor.export(),
    dfIdMap: { toBiz: Object.fromEntries(dfIdToBizId), toDf: Object.fromEntries(bizIdToDfId) },
  };
}

function pushHistory() {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    historyStack.push(currentSnapshot());
    if (historyStack.length > MAX_HISTORY) historyStack.shift();
    redoStack = [];
  }, 300);
}

function clearHistory() {
  historyStack = [];
  redoStack = [];
}

function editorUndo() {
  if (historyStack.length <= 1) return;
  redoStack.push(historyStack.pop());
  restoreSnapshot(historyStack[historyStack.length - 1]);
}

function editorRedo() {
  if (!redoStack.length) return;
  const snap = redoStack.pop();
  historyStack.push(snap);
  restoreSnapshot(snap);
}

function restoreSnapshot(snap) {
  // Restore metadata
  wfMeta = JSON.parse(JSON.stringify(snap.meta));
  document.getElementById('wf-name').value = wfMeta.name;
  document.getElementById('wf-version').value = wfMeta.version;

  // Restore node data
  nodeStore.clear();
  dfIdToBizId.clear();
  bizIdToDfId.clear();
  Object.entries(snap.nodes).forEach(([bizId, data]) => nodeStore.set(bizId, data));

  // Restore canvas
  dfEditor.import(snap.dfData);

  // Restore ID maps
  Object.entries(snap.dfIdMap.toBiz).forEach(([k, v]) => dfIdToBizId.set(k, v));
  Object.entries(snap.dfIdMap.toDf).forEach(([k, v]) => bizIdToDfId.set(k, v));

  hidePropertyPanel();
  updateYamlFromCanvas();
  saveDraft();
}

// ── Toolbar Actions ────────────────────────────────────────────────
function editorNew() {
  if (nodeStore.size > 0) {
    if (!confirm('Discard current workflow and create new?')) return;
  }
  dfEditor.clear();
  nodeStore.clear();
  dfIdToBizId.clear();
  bizIdToDfId.clear();
  nodeIdCounter = 0;
  wfMeta = { name: 'new-workflow', version: '1.0', description: '', timeout: null, defaults: { retry: 0, timeout: 300, shell: 'bash -c' }, inputs: {}, env: {}, references: {} };
  wfBaseDir = null;
  document.getElementById('wf-name').value = wfMeta.name;
  document.getElementById('wf-version').value = wfMeta.version;
  hidePropertyPanel();
  clearHistory();
  pushHistory();
  updateYamlFromCanvas();
  displayValidation([]);
  localStorage.removeItem('acpx-editor-draft');
  highlightEditorTemplate();
}

function editorImport() {
  document.getElementById('import-modal').style.display = 'block';
  document.getElementById('import-yaml-input').value = '';
  document.getElementById('import-yaml-input').focus();
}

function closeImportModal() {
  document.getElementById('import-modal').style.display = 'none';
}

function doImportYaml() {
  const yaml = document.getElementById('import-yaml-input').value;
  if (!yaml.trim()) { showToast('Please paste YAML content', 'error'); return; }
  closeImportModal();
  importFromYaml(yaml);
}

async function editorSave() {
  const errors = validateWorkflow();
  if (errors.length) {
    showToast('Fix validation errors before saving', 'error');
    return;
  }
  const yaml = exportToYaml();
  const name = wfMeta.name || 'untitled';
  try {
    const r = await fetch('/api/v1/templates/save', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, yaml }),
    });
    const result = await r.json();
    if (result.success) {
      showToast(result.message, 'success');
      // Refresh Dashboard template list
      if (typeof loadTemplates === 'function') loadTemplates();
    } else {
      showToast(result.message || 'Save failed', 'error');
    }
  } catch (e) {
    showToast('Network error: ' + e.message, 'error');
  }
}

async function editorRun() {
  const errors = validateWorkflow();
  if (errors.length) {
    showToast('Fix validation errors before running', 'error');
    return;
  }
  const yaml = exportToYaml();
  const payload = { yaml };
  if (wfBaseDir) payload.base_dir = wfBaseDir;
  try {
    const r = await fetch('/api/v1/workflows', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    const result = await r.json();
    if (r.ok) {
      showToast('Workflow started: ' + result.run_id, 'success');
      // Switch to logs view and select the run
      switchTab('logs');
      if (typeof loadRuns === 'function') await loadRuns();
      if (typeof selectRun === 'function') selectRun(result.run_id);
    } else {
      showToast(result.error || 'Failed to start workflow', 'error');
    }
  } catch (e) {
    showToast('Network error: ' + e.message, 'error');
  }
}

function editorDeleteSelected() {
  if (selectedNodeId) {
    dfEditor.removeNodeId('node-' + selectedNodeId);
  }
}

// ── YAML Panel Toggle ──────────────────────────────────────────────
function toggleYamlPanel() {
  const panel = document.getElementById('yaml-panel');
  const wrap = document.getElementById('yaml-editor-wrap');
  const isOpen = panel.classList.toggle('open');
  if (isOpen) {
    wrap.style.display = 'flex';
    // Lazy-init CodeMirror on first open
    if (!cmEditor && typeof CodeMirror !== 'undefined') {
      cmEditor = CodeMirror.fromTextArea(document.getElementById('yaml-code-editor'), {
        mode: 'yaml',
        lineNumbers: true,
        lineWrapping: true,
        tabSize: 2,
        viewportMargin: Infinity,
      });
    }
    if (cmEditor) {
      cmEditor.setValue(exportToYaml());
      setTimeout(() => cmEditor.refresh(), 10);
    }
  } else {
    wrap.style.display = 'none';
  }
}

function copyYaml() {
  const yaml = cmEditor ? cmEditor.getValue() : exportToYaml();
  navigator.clipboard.writeText(yaml).then(() => showToast('YAML copied', 'success'));
}

function downloadYaml() {
  const yaml = cmEditor ? cmEditor.getValue() : exportToYaml();
  const name = (wfMeta.name || 'workflow') + '.yaml';
  const blob = new Blob([yaml], { type: 'text/yaml' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url; a.download = name;
  document.body.appendChild(a); a.click();
  document.body.removeChild(a); URL.revokeObjectURL(url);
}

// ── Workflow Settings ──────────────────────────────────────────────
function addSettingsInputRow(containerId) {
  const container = document.getElementById(containerId);
  if (!container) return;
  const row = document.createElement('div');
  row.className = 'kv-row';
  row.innerHTML = '<input placeholder="name" style="width:80px"><select style="width:70px"><option>string</option><option>number</option><option>boolean</option></select><input placeholder="default"><label style="font-size:10px"><input type="checkbox"> req</label><button class="kv-remove">&times;</button>';
  container.appendChild(row);
}

function addSettingsKvRow(containerId) {
  const container = document.getElementById(containerId);
  if (!container) return;
  const row = document.createElement('div');
  row.className = 'kv-row';
  row.innerHTML = '<input placeholder="KEY"><input placeholder="value"><button class="kv-remove">&times;</button>';
  container.appendChild(row);
}

function showWorkflowSettings() {
  let backdrop = document.getElementById('wf-settings-backdrop');
  let modal = document.getElementById('wf-settings-modal');
  if (!backdrop) {
    backdrop = document.createElement('div');
    backdrop.id = 'wf-settings-backdrop';
    backdrop.className = 'modal-backdrop';
    backdrop.onclick = closeWorkflowSettings;
    document.body.appendChild(backdrop);
  }
  if (!modal) {
    modal = document.createElement('div');
    modal.id = 'wf-settings-modal';
    document.body.appendChild(modal);
  }

  const inputRows = Object.entries(wfMeta.inputs || {}).map(([k, def]) => {
    return `<div class="kv-row">
      <input value="${esc(k)}" placeholder="name" style="width:80px">
      <select style="width:70px"><option ${def.type === 'string' ? 'selected' : ''}>string</option><option ${def.type === 'number' ? 'selected' : ''}>number</option><option ${def.type === 'boolean' ? 'selected' : ''}>boolean</option></select>
      <input value="${esc(def.default || '')}" placeholder="default">
      <label style="font-size:10px"><input type="checkbox" ${def.required ? 'checked' : ''}> req</label>
      <button class="kv-remove" onclick="this.closest('.kv-row').remove()">&times;</button>
    </div>`;
  }).join('');

  const envRows = Object.entries(wfMeta.env || {}).map(([k, v]) => {
    return `<div class="kv-row"><input value="${esc(k)}" placeholder="KEY"><input value="${esc(v)}" placeholder="value"><button class="kv-remove" onclick="this.closest('.kv-row').remove()">&times;</button></div>`;
  }).join('');

  const refRows = Object.entries(wfMeta.references || {}).map(([k, v]) => {
    return `<div class="kv-row"><input value="${esc(k)}" placeholder="alias"><input value="${esc(v)}" placeholder="path/url"><button class="kv-remove" onclick="this.closest('.kv-row').remove()">&times;</button></div>`;
  }).join('');

  modal.innerHTML = `<h3>Workflow Settings</h3>
    <div class="prop-section">
      <div class="prop-section-title">General</div>
      <div class="prop-field"><label>Name</label><input id="ws-name" value="${esc(wfMeta.name)}"></div>
      <div class="prop-field"><label>Version</label><input id="ws-version" value="${esc(wfMeta.version)}"></div>
      <div class="prop-field"><label>Description</label><textarea id="ws-desc" rows="2">${esc(wfMeta.description || '')}</textarea></div>
      <div class="prop-field"><label>Timeout (s)</label><input id="ws-timeout" type="number" value="${wfMeta.timeout || ''}"></div>
    </div>
    <div class="prop-section">
      <div class="prop-section-title">Defaults</div>
      <div class="prop-field"><label>Retry</label><input id="ws-retry" type="number" value="${wfMeta.defaults.retry}"></div>
      <div class="prop-field"><label>Timeout (s)</label><input id="ws-def-timeout" type="number" value="${wfMeta.defaults.timeout}"></div>
      <div class="prop-field"><label>Shell</label><input id="ws-shell" value="${esc(wfMeta.defaults.shell)}"></div>
    </div>
    <div class="prop-section">
      <div class="prop-section-title">Inputs</div>
      <div class="kv-list" id="ws-inputs">${inputRows}</div>
      <button class="kv-add" onclick="addSettingsInputRow('ws-inputs')">+ Add Input</button>
    </div>
    <div class="prop-section">
      <div class="prop-section-title">Environment</div>
      <div class="kv-list" id="ws-env">${envRows}</div>
      <button class="kv-add" onclick="addSettingsKvRow('ws-env')">+ Add</button>
    </div>
    <div class="prop-section">
      <div class="prop-section-title">References</div>
      <div class="kv-list" id="ws-refs">${refRows}</div>
      <button class="kv-add" onclick="addSettingsKvRow('ws-refs')">+ Add</button>
    </div>
    <div style="text-align:right;margin-top:12px">
      <button class="btn btn-sm" onclick="closeWorkflowSettings()">Cancel</button>
      <button class="btn btn-sm btn-primary" onclick="saveWorkflowSettings()">Save Settings</button>
    </div>`;

  backdrop.style.display = 'block';
  modal.style.display = 'block';

  // Event delegation for remove buttons in settings modal
  modal.onclick = function(e) {
    if (e.target.classList.contains('kv-remove')) {
      e.target.closest('.kv-row').remove();
    }
  };
}

function closeWorkflowSettings() {
  const backdrop = document.getElementById('wf-settings-backdrop');
  const modal = document.getElementById('wf-settings-modal');
  if (backdrop) backdrop.style.display = 'none';
  if (modal) modal.style.display = 'none';
}

function saveWorkflowSettings() {
  wfMeta.name = document.getElementById('ws-name')?.value || wfMeta.name;
  wfMeta.version = document.getElementById('ws-version')?.value || wfMeta.version;
  wfMeta.description = document.getElementById('ws-desc')?.value || '';
  wfMeta.timeout = document.getElementById('ws-timeout')?.value ? Number(document.getElementById('ws-timeout').value) : null;
  wfMeta.defaults.retry = Number(document.getElementById('ws-retry')?.value || 0);
  wfMeta.defaults.timeout = Number(document.getElementById('ws-def-timeout')?.value || 300);
  wfMeta.defaults.shell = document.getElementById('ws-shell')?.value || 'bash -c';

  document.getElementById('wf-name').value = wfMeta.name;
  document.getElementById('wf-version').value = wfMeta.version;

  // Parse inputs
  wfMeta.inputs = {};
  document.querySelectorAll('#ws-inputs .kv-row').forEach(row => {
    const inputs = row.querySelectorAll('input');
    const select = row.querySelector('select');
    const name = inputs[0]?.value;
    if (!name) return;
    wfMeta.inputs[name] = {
      type: select?.value || 'string',
      default: inputs[1]?.value || null,
      required: inputs[2]?.type === 'checkbox' ? inputs[2].checked : false,
    };
  });

  // Parse env
  wfMeta.env = {};
  document.querySelectorAll('#ws-env .kv-row').forEach(row => {
    const inputs = row.querySelectorAll('input');
    const k = inputs[0]?.value;
    if (k) wfMeta.env[k] = inputs[1]?.value || '';
  });

  // Parse references
  wfMeta.references = {};
  document.querySelectorAll('#ws-refs .kv-row').forEach(row => {
    const inputs = row.querySelectorAll('input');
    const k = inputs[0]?.value;
    if (k) wfMeta.references[k] = inputs[1]?.value || '';
  });

  closeWorkflowSettings();
  updateYamlFromCanvas();
  pushHistory();
  saveDraft();
  showToast('Settings saved', 'success');
}

// ── Template Loading ───────────────────────────────────────────────
function renderEditorTemplateList(tpls) {
  const el = document.getElementById('editor-template-list');
  if (!el) return;
  if (!tpls || !tpls.length) {
    el.innerHTML = '<div class="template-empty">No templates found</div>';
    return;
  }
  el.innerHTML = tpls.map(t => `
    <div class="template-card${wfMeta.name === t.name ? ' active' : ''}" data-name="${esc(t.name)}">
      <div class="template-name">${esc(t.name)}</div>
      <div class="template-desc">${esc(t.description || 'No description')}</div>
      <div class="template-meta">
        <span class="tag">v${esc(t.version)}</span>
        <span class="tag">${t.node_count} nodes</span>
      </div>
      <div class="template-actions">
        <button class="btn btn-primary btn-sm run-btn" data-name="${esc(t.name)}">&#9654; Run</button>
        <button class="btn btn-sm api-btn" data-name="${esc(t.name)}">API</button>
      </div>
    </div>
  `).join('');

  // Event delegation
  el.querySelectorAll('.template-card').forEach(card => {
    card.addEventListener('click', (e) => {
      if (e.target.closest('.run-btn') || e.target.closest('.api-btn')) return;
      loadTemplateToEditor(card.dataset.name);
    });
  });
  el.querySelectorAll('.run-btn').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      // Run from editor sidebar — validate and submit
      editorRun();
    });
  });
  el.querySelectorAll('.api-btn').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      if (typeof showTemplateApi === 'function') showTemplateApi(btn.dataset.name);
    });
  });
}

function refreshEditorTemplateList() {
  if (typeof allTemplates === 'undefined') return;
  renderEditorTemplateList(allTemplates);
}

function highlightEditorTemplate() {
  document.querySelectorAll('#editor-template-list .template-card').forEach(card => {
    card.classList.toggle('active', card.dataset.name === wfMeta.name);
  });
}

async function loadTemplateToEditor(name) {
  if (!name) return;
  try {
    const r = await fetch(`/api/v1/templates/${encodeURIComponent(name)}/yaml`);
    const result = await r.json();
    if (result.yaml) {
      importFromYaml(result.yaml);
      // Derive base directory from template file path for reference resolution
      if (typeof allTemplates !== 'undefined') {
        const tpl = allTemplates.find(t => t.name === name);
        if (tpl && tpl.file_path) {
          const parts = tpl.file_path.replace(/\\/g, '/').split('/');
          parts.pop(); // remove filename
          wfBaseDir = parts.length ? parts.join('/') + '/' : null;
        } else {
          wfBaseDir = null;
        }
      }
      highlightEditorTemplate();
    } else {
      showToast(result.error || 'Failed to load template', 'error');
    }
  } catch (e) {
    showToast('Network error: ' + e.message, 'error');
  }
}

// Listen for workflow metadata inputs
document.addEventListener('DOMContentLoaded', function() {
  // Editor is now the default view — initialize after layout is computed
  requestAnimationFrame(() => { requestAnimationFrame(() => { initEditor(); }); });

  const nameInput = document.getElementById('wf-name');
  const verInput = document.getElementById('wf-version');
  if (nameInput) nameInput.addEventListener('change', function() {
    wfMeta.name = this.value || 'untitled';
    highlightEditorTemplate();
    updateYamlFromCanvas();
    saveDraft();
  });
  if (verInput) verInput.addEventListener('change', function() {
    wfMeta.version = this.value || '1.0';
    updateYamlFromCanvas();
    saveDraft();
  });
});

// ── Auto-Save to localStorage ──────────────────────────────────────
function saveDraft() {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    try {
      localStorage.setItem('acpx-editor-draft', JSON.stringify({
        meta: wfMeta,
        nodes: Object.fromEntries(nodeStore),
        canvas: dfEditor?.export(),
        idMap: { toBiz: Object.fromEntries(dfIdToBizId), toDf: Object.fromEntries(bizIdToDfId) },
        counter: nodeIdCounter,
        ts: Date.now(),
      }));
    } catch (e) { /* localStorage full, ignore */ }
  }, 1000);
}

function loadDraft() {
  try {
    const raw = localStorage.getItem('acpx-editor-draft');
    if (!raw) return;
    const draft = JSON.parse(raw);
    if (!draft.nodes || !draft.canvas) return;

    // Restore metadata
    wfMeta = draft.meta || wfMeta;
    document.getElementById('wf-name').value = wfMeta.name;
    document.getElementById('wf-version').value = wfMeta.version;

    // Restore node store
    nodeStore.clear();
    Object.entries(draft.nodes).forEach(([k, v]) => nodeStore.set(k, v));

    // Restore canvas
    if (dfEditor && draft.canvas) dfEditor.import(draft.canvas);

    // Restore ID maps
    dfIdToBizId.clear();
    bizIdToDfId.clear();
    if (draft.idMap) {
      Object.entries(draft.idMap.toBiz || {}).forEach(([k, v]) => dfIdToBizId.set(k, v));
      Object.entries(draft.idMap.toDf || {}).forEach(([k, v]) => bizIdToDfId.set(k, v));
    }

    nodeIdCounter = draft.counter || nodeStore.size;
    updateYamlFromCanvas();
    displayValidation([]);
    pushHistory();
  } catch (e) { /* corrupt draft, ignore */ }
}

// ── Utility Functions ──────────────────────────────────────────────
function esc(s) {
  return String(s || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}
