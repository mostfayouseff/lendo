const API = '/api/v1';
let ws = null;
let authToken = null;
let currentPage = 'overview';

const state = {
  user:       null,
  trades:     [],
  strategies: [],
  tokens:     [],
  wallets:    [],
  overview:   null,
  events:     [],
};

async function apiFetch(path, opts = {}) {
  const headers = { 'Content-Type': 'application/json', ...opts.headers };
  if (authToken) headers['Authorization'] = `Bearer ${authToken}`;
  const res = await fetch(API + path, { ...opts, headers });
  if (res.status === 401) { logout(); return null; }
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.error?.message || `HTTP ${res.status}`);
  }
  return res.status === 204 ? null : res.json();
}

function formatLamports(v) {
  if (v == null) return '—';
  const sol = v / 1e9;
  return sol >= 1 ? sol.toFixed(4) + ' SOL' : (v / 1000).toFixed(0) + 'K lamps';
}

function formatDate(d) {
  return d ? new Date(d).toLocaleString() : '—';
}

function statusBadge(s) {
  const map = {
    confirmed: 'badge-green', active: 'badge-green', ok: 'badge-green',
    failed: 'badge-red', disabled: 'badge-red', suspended: 'badge-red',
    pending: 'badge-yellow', simulating: 'badge-yellow', paused: 'badge-yellow',
    submitted: 'badge-blue', submitted: 'badge-blue', detected: 'badge-blue',
  };
  return `<span class="${map[s] || 'badge badge-blue'}">${s}</span>`;
}

async function login(email, password) {
  const data = await apiFetch('/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  });
  if (!data) return;
  authToken = data.tokens.access_token;
  localStorage.setItem('apex_token',  authToken);
  localStorage.setItem('apex_refresh', data.tokens.refresh_token);
  state.user = data.user;
  showApp();
}

function logout() {
  authToken = null;
  localStorage.removeItem('apex_token');
  localStorage.removeItem('apex_refresh');
  state.user = null;
  if (ws) { ws.close(); ws = null; }
  showLogin();
}

function connectWs() {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  ws = new WebSocket(`${proto}//${location.host}/ws`);
  ws.onmessage = (e) => {
    const ev = JSON.parse(e.data);
    handleWsEvent(ev);
  };
  ws.onclose = () => setTimeout(connectWs, 3000);
}

function handleWsEvent(ev) {
  const toast = document.getElementById('toast');
  if (!toast) return;
  const map = {
    trade_executed:    () => { toast.innerHTML = `<span class="badge-green badge">Trade</span> ${ev.status} — ${formatLamports(ev.profit_lamports)}`; },
    bot_status:        () => { toast.innerHTML = `<span class="badge-blue badge">Bot</span> ${ev.running ? '▶ Running' : '⏹ Stopped'} (${ev.mode})`; updateBotBadge(ev.running); },
    system_alert:      () => { toast.innerHTML = `<span class="badge-red badge">${ev.severity.toUpperCase()}</span> ${ev.message}`; },
    circuit_breaker:   () => { toast.innerHTML = `<span class="badge-red badge">CIRCUIT BREAKER</span> ${ev.reason}`; },
    opportunity_found: () => { toast.innerHTML = `<span class="badge-yellow badge">Opportunity</span> +${formatLamports(ev.profit_lamports)}`; },
  };
  if (map[ev.type]) { map[ev.type](); toast.classList.remove('opacity-0'); setTimeout(() => toast.classList.add('opacity-0'), 4000); }
}

function updateBotBadge(running) {
  const b = document.getElementById('bot-status-badge');
  if (b) { b.className = running ? 'badge badge-green' : 'badge badge-red'; b.textContent = running ? '● LIVE' : '● STOPPED'; }
}

function showLogin() {
  document.body.innerHTML = `
    <div class="min-h-screen flex items-center justify-center p-4">
      <div class="card w-full max-w-md">
        <div class="text-center mb-8">
          <h1 class="text-3xl font-bold text-apex-400">⚡ Apex MEV</h1>
          <p class="text-gray-400 mt-2 text-sm">Flash Loan Arbitrage Platform</p>
        </div>
        <div class="space-y-4" id="login-form">
          <div>
            <label class="block text-sm text-gray-400 mb-1">Email</label>
            <input id="email" type="email" class="input" placeholder="admin@apex.local" />
          </div>
          <div>
            <label class="block text-sm text-gray-400 mb-1">Password</label>
            <input id="password" type="password" class="input" placeholder="••••••••" />
          </div>
          <p id="login-error" class="text-red-400 text-sm hidden"></p>
          <button onclick="doLogin()" class="btn-primary w-full text-center py-3">Sign In</button>
        </div>
      </div>
    </div>`;
  document.getElementById('password').addEventListener('keydown', e => { if (e.key === 'Enter') doLogin(); });
}

async function doLogin() {
  const email = document.getElementById('email').value;
  const pass  = document.getElementById('password').value;
  const err   = document.getElementById('login-error');
  try {
    await login(email, pass);
  } catch (e) {
    err.textContent = e.message;
    err.classList.remove('hidden');
  }
}

function showApp() {
  connectWs();
  document.body.innerHTML = `
    <div class="flex h-screen overflow-hidden">
      <!-- Sidebar -->
      <aside class="w-64 bg-apex-800 border-r border-apex-700 flex flex-col">
        <div class="p-6 border-b border-apex-700">
          <h1 class="text-xl font-bold text-apex-400">⚡ Apex MEV</h1>
          <p class="text-xs text-gray-500 mt-1">Neural Core 3.0</p>
        </div>
        <div class="p-4 border-b border-apex-700 flex items-center justify-between">
          <span id="bot-status-badge" class="badge badge-red">● STOPPED</span>
          <button onclick="sendBotCommand('start')" class="btn-primary text-xs px-2 py-1">▶ Start</button>
          <button onclick="sendBotCommand('stop')"  class="btn-ghost text-xs px-2 py-1">⏹</button>
        </div>
        <nav class="flex-1 p-4 space-y-1">
          ${[
            ['overview','📊','Overview'],
            ['trades','💹','Trades'],
            ['opportunities','🎯','Opportunities'],
            ['strategies','🤖','Strategies'],
            ['flash-loans','⚡','Flash Loans'],
            ['wallets','👛','Wallets'],
            ['tokens','🪙','Tokens'],
            ['risk','🛡️','Risk Rules'],
            ['settings','⚙️','Settings'],
            ['logs','📋','Audit Logs'],
          ].map(([page, icon, label]) => `
            <button onclick="navigate('${page}')" id="nav-${page}"
              class="w-full text-left flex items-center gap-3 px-3 py-2 rounded-lg text-sm text-gray-400 hover:bg-apex-700 hover:text-white transition-colors nav-item">
              ${icon} ${label}
            </button>`).join('')}
        </nav>
        <div class="p-4 border-t border-apex-700">
          <div class="text-xs text-gray-500">${state.user?.email || ''}</div>
          <button onclick="logout()" class="btn-ghost text-xs mt-2 w-full">Sign Out</button>
        </div>
      </aside>

      <!-- Main Content -->
      <main class="flex-1 overflow-auto">
        <div class="sticky top-0 z-10 bg-apex-900/80 backdrop-blur border-b border-apex-700 px-6 py-3 flex items-center justify-between">
          <h2 id="page-title" class="text-lg font-semibold text-white">Overview</h2>
          <div id="toast" class="text-sm px-3 py-1 bg-apex-800 rounded-lg opacity-0 transition-opacity duration-500"></div>
        </div>
        <div id="page-content" class="p-6"></div>
      </main>
    </div>`;

  navigate('overview');
}

async function navigate(page) {
  currentPage = page;
  document.querySelectorAll('.nav-item').forEach(el => el.classList.remove('bg-apex-700', 'text-white'));
  const active = document.getElementById(`nav-${page}`);
  if (active) active.classList.add('bg-apex-700', 'text-white');
  document.getElementById('page-title').textContent = {
    overview: 'Overview', trades: 'Trade History', opportunities: 'Opportunities',
    strategies: 'Strategies', 'flash-loans': 'Flash Loans', wallets: 'Wallets',
    tokens: 'Token Universe', risk: 'Risk Rules', settings: 'Settings', logs: 'Audit Logs',
  }[page] || page;
  const pages = {
    overview:       renderOverview,
    trades:         renderTrades,
    opportunities:  renderOpportunities,
    strategies:     renderStrategies,
    'flash-loans':  renderFlashLoans,
    wallets:        renderWallets,
    tokens:         renderTokens,
    risk:           renderRisk,
    settings:       renderSettings,
    logs:           renderLogs,
  };
  const fn = pages[page];
  if (fn) { document.getElementById('page-content').innerHTML = '<div class="text-center text-gray-400 py-20">Loading…</div>'; await fn(); }
}

async function sendBotCommand(cmd) {
  try {
    const r = await apiFetch('/bot/command', { method: 'POST', body: JSON.stringify({ command: cmd }) });
    if (r) showToast(`Bot ${cmd} executed`);
  } catch(e) { showToast(`Error: ${e.message}`, true); }
}

function showToast(msg, err = false) {
  const t = document.getElementById('toast');
  if (!t) return;
  t.innerHTML = `<span class="${err ? 'badge-red' : 'badge-green'} badge">${err ? '✗' : '✓'}</span> ${msg}`;
  t.classList.remove('opacity-0');
  setTimeout(() => t.classList.add('opacity-0'), 3000);
}

async function renderOverview() {
  const [overview, botStatus] = await Promise.all([
    apiFetch('/monitoring/overview'),
    apiFetch('/bot/status'),
  ]).catch(() => [null, null]);

  const d = overview || {};
  const bot = botStatus || {};

  document.getElementById('page-content').innerHTML = `
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
      ${[
        ['Total Trades',    d.total_trades || 0,       ''],
        ['Confirmed',       d.confirmed_trades || 0,   'text-emerald-400'],
        ['Win Rate',        ((d.win_rate||0)*100).toFixed(1)+'%', (d.win_rate||0) > 0.6 ? 'text-emerald-400' : 'text-yellow-400'],
        ['Total Profit',    formatLamports((d.total_profit_sol||0)*1e9), 'text-emerald-400'],
        ['Opportunities',   d.opportunities_today || 0, ''],
        ['Active Strategies', d.active_strategies || 0, ''],
        ['Open Alerts',     d.unresolved_alerts || 0,  (d.unresolved_alerts||0)>0?'text-red-400':''],
        ['Bot Status',      bot.running ? '▶ LIVE' : '⏹ STOPPED', bot.running ? 'text-emerald-400':'text-red-400'],
      ].map(([label, val, cls]) => `
        <div class="card">
          <div class="stat-value ${cls}">${val}</div>
          <div class="stat-label">${label}</div>
        </div>`).join('')}
    </div>
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
      <div class="card">
        <h3 class="text-sm font-semibold text-gray-300 mb-4">Recent System Events</h3>
        <div id="events-list"><div class="text-gray-500 text-sm">Loading…</div></div>
      </div>
      <div class="card">
        <h3 class="text-sm font-semibold text-gray-300 mb-4">Bot Configuration</h3>
        <div class="space-y-2 text-sm">
          <div class="flex justify-between"><span class="text-gray-400">Mode</span><span class="font-mono">${bot.mode || '—'}</span></div>
          <div class="flex justify-between"><span class="text-gray-400">Uptime</span><span>${bot.uptime_sec || 0}s</span></div>
        </div>
      </div>
    </div>`;

  const evs = await apiFetch('/monitoring/events').catch(() => []);
  const evList = document.getElementById('events-list');
  if (evList) {
    evList.innerHTML = (evs||[]).slice(0,5).map(e => `
      <div class="flex items-start gap-2 py-2 border-b border-apex-700 last:border-0">
        <span class="${e.severity==='critical'||e.severity==='error'?'badge-red':e.severity==='warning'?'badge-yellow':'badge-blue'} badge">${e.severity}</span>
        <div class="flex-1 min-w-0">
          <div class="text-sm truncate">${e.title}</div>
          <div class="text-xs text-gray-500">${formatDate(e.created_at)}</div>
        </div>
      </div>`).join('') || '<div class="text-gray-500 text-sm">No alerts</div>';
  }
}

async function renderTrades() {
  const [trades, summary] = await Promise.all([
    apiFetch('/trades?limit=50'),
    apiFetch('/trades/summary'),
  ]);
  document.getElementById('page-content').innerHTML = `
    <div class="grid grid-cols-4 gap-4 mb-6">
      ${[
        ['Total',    summary?.total_trades || 0,    ''],
        ['Confirmed', summary?.confirmed_trades || 0, 'text-emerald-400'],
        ['Failed',   summary?.failed_trades || 0,   'text-red-400'],
        ['Total Profit', (summary?.total_profit_sol||0).toFixed(4)+' SOL', 'text-emerald-400'],
      ].map(([l,v,c])=>`<div class="card"><div class="stat-value ${c}">${v}</div><div class="stat-label">${l}</div></div>`).join('')}
    </div>
    <div class="card overflow-auto">
      <table class="w-full text-sm">
        <thead>
          <tr class="text-gray-400 text-left border-b border-apex-700">
            <th class="py-3 pr-4">Status</th>
            <th class="py-3 pr-4">Input → Output</th>
            <th class="py-3 pr-4">Expected Profit</th>
            <th class="py-3 pr-4">Actual Profit</th>
            <th class="py-3 pr-4">Hops</th>
            <th class="py-3 pr-4">Created</th>
            <th class="py-3">Signature</th>
          </tr>
        </thead>
        <tbody>
          ${(trades||[]).map(t=>`
            <tr class="table-row">
              <td class="py-3 pr-4">${statusBadge(t.status)}</td>
              <td class="py-3 pr-4 font-mono text-xs">${t.input_mint.slice(0,8)}…→${t.output_mint.slice(0,8)}…</td>
              <td class="py-3 pr-4">${formatLamports(t.expected_profit_lamports)}</td>
              <td class="py-3 pr-4 ${(t.actual_profit_lamports||0)>0?'text-emerald-400':'text-red-400'}">${formatLamports(t.actual_profit_lamports)}</td>
              <td class="py-3 pr-4">${t.hop_count}</td>
              <td class="py-3 pr-4 text-gray-400 text-xs">${formatDate(t.created_at)}</td>
              <td class="py-3 text-xs font-mono text-gray-500">${t.signature?t.signature.slice(0,12)+'…':'—'}</td>
            </tr>`).join('') || '<tr><td colspan="7" class="py-8 text-center text-gray-500">No trades yet</td></tr>'}
        </tbody>
      </table>
    </div>`;
}

async function renderOpportunities() {
  const data = await apiFetch('/opportunities/recent').catch(() => []);
  document.getElementById('page-content').innerHTML = `
    <div class="card overflow-auto">
      <table class="w-full text-sm">
        <thead>
          <tr class="text-gray-400 text-left border-b border-apex-700">
            <th class="py-3 pr-4">Status</th>
            <th class="py-3 pr-4">Path</th>
            <th class="py-3 pr-4">Est. Profit</th>
            <th class="py-3 pr-4">Hops</th>
            <th class="py-3 pr-4">Confidence</th>
            <th class="py-3">Detected</th>
          </tr>
        </thead>
        <tbody>
          ${(data||[]).map(o=>`
            <tr class="table-row">
              <td class="py-3 pr-4">${statusBadge(o.status)}</td>
              <td class="py-3 pr-4 text-xs font-mono">${(o.path||[]).join(' → ')}</td>
              <td class="py-3 pr-4 text-emerald-400">${formatLamports(o.estimated_profit_lamports)}</td>
              <td class="py-3 pr-4">${o.hop_count}</td>
              <td class="py-3 pr-4">${o.gnn_confidence ? (parseFloat(o.gnn_confidence)*100).toFixed(1)+'%' : '—'}</td>
              <td class="py-3 text-gray-400 text-xs">${formatDate(o.detected_at)}</td>
            </tr>`).join('') || '<tr><td colspan="6" class="py-8 text-center text-gray-500">No opportunities detected yet</td></tr>'}
        </tbody>
      </table>
    </div>`;
}

async function renderStrategies() {
  const data = await apiFetch('/strategies').catch(() => []);
  document.getElementById('page-content').innerHTML = `
    <div class="flex justify-end mb-4">
      <button onclick="showCreateStrategy()" class="btn-primary">+ New Strategy</button>
    </div>
    <div class="grid gap-4">
      ${(data||[]).map(s=>`
        <div class="card flex items-start justify-between">
          <div>
            <div class="flex items-center gap-3">
              <span class="font-semibold">${s.name}</span>
              ${statusBadge(s.status)}
              <span class="badge badge-blue">${s.strategy_type}</span>
            </div>
            <div class="mt-2 grid grid-cols-3 gap-4 text-sm text-gray-400">
              <span>Min profit: ${formatLamports(s.min_profit_lamports)}</span>
              <span>Max pos: ${formatLamports(s.max_position_lamports)}</span>
              <span>Hops: ${s.max_hops}</span>
              <span>Trades: ${s.trades_executed}</span>
              <span>Profit: ${formatLamports(s.total_profit_lamports)}</span>
              <span>Flash loan: ${s.flash_loan_enabled ? '✓ '+s.flash_loan_provider : '✗'}</span>
            </div>
          </div>
          <div class="flex gap-2">
            ${s.status !== 'active'  ? `<button onclick="strategyAction('${s.id}','start')" class="btn-primary text-xs">▶ Start</button>` : ''}
            ${s.status === 'active'  ? `<button onclick="strategyAction('${s.id}','pause')" class="btn-ghost text-xs">⏸ Pause</button>` : ''}
            <button onclick="strategyAction('${s.id}','delete')" class="btn-danger text-xs">🗑</button>
          </div>
        </div>`).join('') || '<div class="card text-center text-gray-500 py-12">No strategies configured</div>'}
    </div>`;
}

async function strategyAction(id, action) {
  try {
    if (action === 'delete' && !confirm('Delete this strategy?')) return;
    const map = { start: ['POST', `/strategies/${id}/start`], pause: ['POST', `/strategies/${id}/pause`], delete: ['DELETE', `/strategies/${id}`] };
    const [method, path] = map[action];
    await apiFetch(path, { method });
    await renderStrategies();
  } catch(e) { showToast(e.message, true); }
}

async function renderFlashLoans() {
  const [providers] = await Promise.all([apiFetch('/flash-loans/providers')]);
  document.getElementById('page-content').innerHTML = `
    <div class="grid grid-cols-3 gap-4 mb-6">
      ${(providers?.providers||[]).map(p=>`
        <div class="card">
          <div class="text-lg font-bold text-apex-400">${p.name.toUpperCase()}</div>
          <div class="text-sm text-gray-400 mt-1">${p.description}</div>
          <div class="mt-3 text-2xl font-mono">${p.fee_bps} bps</div>
          <div class="text-xs text-gray-500">Fee</div>
        </div>`).join('')}
    </div>
    <div class="card">
      <h3 class="text-sm font-semibold text-gray-300 mb-4">Quote Flash Loan</h3>
      <div class="grid grid-cols-3 gap-4 mb-4">
        <div>
          <label class="block text-xs text-gray-400 mb-1">Provider</label>
          <select id="fl-provider" class="input text-sm">
            <option>solend</option><option>marginfi</option><option>kamino</option>
          </select>
        </div>
        <div>
          <label class="block text-xs text-gray-400 mb-1">Borrow Mint</label>
          <input id="fl-mint" class="input text-sm" value="So11111111111111111111111111111111111111112" />
        </div>
        <div>
          <label class="block text-xs text-gray-400 mb-1">Amount (lamports)</label>
          <input id="fl-amount" class="input text-sm" type="number" value="1000000000" />
        </div>
      </div>
      <button onclick="getFlashLoanQuote()" class="btn-primary">Get Quote</button>
      <div id="fl-result" class="mt-4"></div>
    </div>`;
}

async function getFlashLoanQuote() {
  const body = {
    provider:      document.getElementById('fl-provider').value,
    borrow_mint:   document.getElementById('fl-mint').value,
    borrow_amount: parseInt(document.getElementById('fl-amount').value),
  };
  try {
    const q = await apiFetch('/flash-loans/quote', { method: 'POST', body: JSON.stringify(body) });
    document.getElementById('fl-result').innerHTML = `
      <div class="grid grid-cols-3 gap-4 mt-2 p-4 bg-apex-700 rounded-lg text-sm">
        <div><div class="text-gray-400">Borrow</div><div>${formatLamports(q.borrow_amount)}</div></div>
        <div><div class="text-gray-400">Fee (${q.fee_bps} bps)</div><div class="text-red-400">${formatLamports(q.fee_amount)}</div></div>
        <div><div class="text-gray-400">Repay</div><div>${formatLamports(q.repay_amount)}</div></div>
        <div><div class="text-gray-400">Available</div><div class="${q.available?'text-emerald-400':'text-red-400'}">${q.available?'✓ Yes':'✗ No'}</div></div>
      </div>`;
  } catch(e) { showToast(e.message, true); }
}

async function renderWallets() {
  const data = await apiFetch('/wallets').catch(() => []);
  document.getElementById('page-content').innerHTML = `
    <div class="grid gap-4">
      ${(data||[]).map(w=>`
        <div class="card flex items-start justify-between">
          <div>
            <div class="flex items-center gap-3">
              <span class="font-semibold">${w.label}</span>
              ${statusBadge(w.status)}
              ${w.is_active ? '<span class="badge badge-green">ACTIVE</span>' : ''}
            </div>
            <div class="font-mono text-xs text-gray-400 mt-1">${w.address}</div>
            <div class="mt-2 text-sm text-gray-400">Balance: <span class="text-white">${w.balance_sol?.toFixed(4)||'0.0000'} SOL</span></div>
          </div>
          <div class="flex gap-2">
            ${!w.is_active ? `<button onclick="activateWallet('${w.id}')" class="btn-primary text-xs">Activate</button>` : ''}
            <button onclick="deleteWallet('${w.id}','${w.is_active}')" class="btn-danger text-xs">🗑</button>
          </div>
        </div>`).join('') || '<div class="card text-center text-gray-500 py-12">No wallets configured</div>'}
    </div>`;
}

async function activateWallet(id) {
  try { await apiFetch(`/wallets/${id}/activate`, {method:'POST'}); await renderWallets(); }
  catch(e) { showToast(e.message, true); }
}

async function deleteWallet(id, isActive) {
  if (isActive === 'true') { showToast('Cannot delete the active wallet', true); return; }
  if (!confirm('Delete this wallet?')) return;
  try { await apiFetch(`/wallets/${id}`, {method:'DELETE'}); await renderWallets(); }
  catch(e) { showToast(e.message, true); }
}

async function renderTokens() {
  const data = await apiFetch('/tokens').catch(() => []);
  document.getElementById('page-content').innerHTML = `
    <div class="card overflow-auto">
      <table class="w-full text-sm">
        <thead>
          <tr class="text-gray-400 text-left border-b border-apex-700">
            <th class="py-3 pr-4">Symbol</th>
            <th class="py-3 pr-4">Name</th>
            <th class="py-3 pr-4">Mint</th>
            <th class="py-3 pr-4">Decimals</th>
            <th class="py-3 pr-4">Status</th>
            <th class="py-3">Actions</th>
          </tr>
        </thead>
        <tbody>
          ${(data||[]).map(t=>`
            <tr class="table-row">
              <td class="py-3 pr-4 font-bold">${t.symbol}</td>
              <td class="py-3 pr-4 text-gray-400">${t.name}</td>
              <td class="py-3 pr-4 font-mono text-xs text-gray-500">${t.mint_address.slice(0,12)}…</td>
              <td class="py-3 pr-4">${t.decimals}</td>
              <td class="py-3 pr-4">${statusBadge(t.status)}</td>
              <td class="py-3">
                ${t.status==='active' ?
                  `<button onclick="setTokenStatus('${t.id}','disabled')" class="btn-ghost text-xs">Disable</button>` :
                  `<button onclick="setTokenStatus('${t.id}','active')"   class="btn-primary text-xs">Enable</button>`}
              </td>
            </tr>`).join('') || '<tr><td colspan="6" class="py-8 text-center text-gray-500">No tokens</td></tr>'}
        </tbody>
      </table>
    </div>`;
}

async function setTokenStatus(id, status) {
  try { await apiFetch(`/tokens/${id}/status/${status}`, {method:'POST'}); await renderTokens(); }
  catch(e) { showToast(e.message, true); }
}

async function renderRisk() {
  const data = await apiFetch('/risk/rules').catch(() => []);
  document.getElementById('page-content').innerHTML = `
    <div class="grid gap-4">
      ${(data||[]).map(r=>`
        <div class="card flex items-start justify-between">
          <div>
            <div class="flex items-center gap-3">
              <span class="font-semibold">${r.name}</span>
              ${r.enabled ? '<span class="badge badge-green">ENABLED</span>' : '<span class="badge badge-red">DISABLED</span>'}
              <span class="badge badge-blue">${r.rule_type}</span>
            </div>
            ${r.description ? `<div class="text-sm text-gray-400 mt-1">${r.description}</div>` : ''}
            <div class="mt-2 text-xs font-mono text-gray-500">${JSON.stringify(r.config)}</div>
          </div>
          <button onclick="toggleRiskRule('${r.id}',${r.enabled})" class="${r.enabled?'btn-danger':'btn-primary'} text-xs">
            ${r.enabled ? 'Disable' : 'Enable'}
          </button>
        </div>`).join('') || '<div class="card text-center text-gray-500 py-12">No risk rules</div>'}
    </div>`;
}

async function toggleRiskRule(id, enabled) {
  try { await apiFetch(`/risk/rules/${id}`, {method:'PUT', body:JSON.stringify({enabled:!enabled})}); await renderRisk(); }
  catch(e) { showToast(e.message, true); }
}

async function renderSettings() {
  const bot = await apiFetch('/settings/bot').catch(() => ({}));
  const b = bot || {};
  document.getElementById('page-content').innerHTML = `
    <div class="grid grid-cols-2 gap-6">
      <div class="card">
        <h3 class="text-sm font-semibold text-gray-300 mb-4">Bot Configuration</h3>
        <div class="space-y-4">
          ${[
            ['mode', 'Mode', b.mode||'test'],
            ['min_profit_lamports', 'Min Profit (lamports)', b.min_profit_lamports||10000],
            ['max_position_lamports', 'Max Position (lamports)', b.max_position_lamports||1000000000],
            ['slippage_bps', 'Slippage (bps)', b.slippage_bps||50],
            ['max_hops', 'Max Hops', b.max_hops||4],
            ['jito_tip_lamports', 'Jito Tip (lamports)', b.jito_tip_lamports||1000],
          ].map(([key,label,val])=>`
            <div>
              <label class="block text-xs text-gray-400 mb-1">${label}</label>
              <input id="setting-${key}" class="input text-sm" value="${val}" />
            </div>`).join('')}
          <div class="flex items-center gap-3">
            <input type="checkbox" id="setting-flash_loan_enabled" ${b.flash_loan_enabled?'checked':''} class="rounded" />
            <label class="text-sm text-gray-300">Enable Flash Loans</label>
          </div>
          <button onclick="saveBotSettings()" class="btn-primary w-full">Save Settings</button>
        </div>
      </div>
      <div class="card">
        <h3 class="text-sm font-semibold text-gray-300 mb-4">Bot Control</h3>
        <div class="space-y-3">
          <button onclick="sendBotCommand('start')" class="btn-primary w-full py-3">▶ Start Bot</button>
          <button onclick="sendBotCommand('stop')"  class="btn-ghost w-full py-3">⏹ Stop Bot</button>
          <button onclick="sendBotCommand('emergency_stop')" class="btn-danger w-full py-3">🚨 Emergency Stop</button>
        </div>
      </div>
    </div>`;
}

async function saveBotSettings() {
  const get = id => document.getElementById(`setting-${id}`)?.value;
  const body = {
    enabled:               false,
    mode:                  get('mode'),
    min_profit_lamports:   parseInt(get('min_profit_lamports')),
    max_position_lamports: parseInt(get('max_position_lamports')),
    slippage_bps:          parseInt(get('slippage_bps')),
    max_hops:              parseInt(get('max_hops')),
    flash_loan_enabled:    document.getElementById('setting-flash_loan_enabled')?.checked,
    flash_loan_provider:   'solend',
    jito_tip_lamports:     parseInt(get('jito_tip_lamports')),
  };
  try { await apiFetch('/settings/bot', {method:'PUT', body:JSON.stringify(body)}); showToast('Settings saved'); }
  catch(e) { showToast(e.message, true); }
}

async function renderLogs() {
  const data = await apiFetch('/monitoring/events?limit=50').catch(() => []);
  document.getElementById('page-content').innerHTML = `
    <div class="card overflow-auto">
      <table class="w-full text-sm">
        <thead>
          <tr class="text-gray-400 text-left border-b border-apex-700">
            <th class="py-3 pr-4">Severity</th>
            <th class="py-3 pr-4">Category</th>
            <th class="py-3 pr-4">Title</th>
            <th class="py-3 pr-4">Status</th>
            <th class="py-3 pr-4">Created</th>
            <th class="py-3">Actions</th>
          </tr>
        </thead>
        <tbody>
          ${(data||[]).map(e=>`
            <tr class="table-row">
              <td class="py-3 pr-4">${statusBadge(e.severity)}</td>
              <td class="py-3 pr-4 text-gray-400">${e.category}</td>
              <td class="py-3 pr-4">${e.title}</td>
              <td class="py-3 pr-4">${e.resolved ? '<span class="badge badge-green">resolved</span>' : '<span class="badge badge-yellow">open</span>'}</td>
              <td class="py-3 pr-4 text-xs text-gray-400">${formatDate(e.created_at)}</td>
              <td class="py-3">
                ${!e.resolved ? `<button onclick="resolveEvent('${e.id}')" class="btn-ghost text-xs">Resolve</button>` : ''}
              </td>
            </tr>`).join('') || '<tr><td colspan="6" class="py-8 text-center text-gray-500">No events</td></tr>'}
        </tbody>
      </table>
    </div>`;
}

async function resolveEvent(id) {
  try { await apiFetch(`/monitoring/events/${id}/resolve`, {method:'POST'}); await renderLogs(); }
  catch(e) { showToast(e.message, true); }
}

(async function init() {
  authToken = localStorage.getItem('apex_token');
  if (authToken) {
    try {
      const me = await apiFetch('/users/me');
      if (me) { state.user = me; showApp(); return; }
    } catch(e) { authToken = null; }
  }
  showLogin();
})();
