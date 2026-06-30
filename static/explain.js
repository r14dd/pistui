(function () {
  'use strict';

  var PROVIDERS = [
    { id: 'openai', name: 'OpenAI', baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o-mini', needsKey: true },
    { id: 'anthropic', name: 'Anthropic', baseUrl: 'https://api.anthropic.com/v1', model: 'claude-haiku-4-5-20251001', needsKey: true },
    { id: 'google', name: 'Google AI', baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', model: 'gemini-2.0-flash-lite', needsKey: true },
    { id: 'groq', name: 'Groq', baseUrl: 'https://api.groq.com/openai/v1', model: 'llama-3.3-70b-versatile', needsKey: true },
    { id: 'ollama', name: 'Ollama', baseUrl: 'http://localhost:11434/v1', model: 'llama3.2', needsKey: false },
    { id: 'custom', name: 'Other', baseUrl: '', model: '', needsKey: true },
  ];

  var STORAGE_KEY = 'pistui_llm_config';

  function loadConfig() {
    try {
      var raw = localStorage.getItem(STORAGE_KEY);
      return raw ? JSON.parse(raw) : null;
    } catch (e) {
      return null;
    }
  }

  function saveConfig(config) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
  }

  function clearConfig() {
    localStorage.removeItem(STORAGE_KEY);
  }

  window.hasExplainConfig = function () {
    return loadConfig() !== null;
  };

  window.renderExplainButton = function (container, traceId) {
    var wrapper = el('div', 'explain-section');

    var config = loadConfig();

    var promptInput = el('input', 'explain-prompt');
    promptInput.type = 'text';
    promptInput.placeholder = 'focus on errors, explain in Spanish, why so expensive…';
    wrapper.appendChild(promptInput);

    var btn = el('button', 'btn explain-btn', 'explain');
    btn.addEventListener('click', function () {
      var cfg = loadConfig();
      if (cfg) {
        runExplain(wrapper, traceId, cfg, promptInput.value.trim());
      } else {
        showProviderPicker(wrapper, traceId);
      }
    });
    wrapper.appendChild(btn);

    if (config) {
      var provider = PROVIDERS.find(function (p) { return p.id === config.providerId; });
      var label = (provider ? provider.name : 'Custom') + ' / ' + config.model;
      var settings = el('span', 'explain-settings-link', label);
      settings.title = 'Change LLM settings';
      settings.addEventListener('click', function () {
        showProviderPicker(wrapper, traceId);
      });
      wrapper.appendChild(settings);
    }

    container.appendChild(wrapper);
  };

  function showProviderPicker(wrapper, traceId) {
    var existing = wrapper.querySelector('.explain-picker');
    if (existing) existing.remove();
    var existingResult = wrapper.querySelector('.explain-result');
    if (existingResult) existingResult.remove();

    var picker = el('div', 'explain-picker');

    var title = el('div', 'picker-title', 'Which do you use?');
    var subtitle = el('div', 'picker-subtitle', 'Any model works, pick whatever you have.');
    picker.appendChild(title);
    picker.appendChild(subtitle);

    var grid = el('div', 'provider-grid');

    PROVIDERS.forEach(function (prov) {
      var card = el('div', 'provider-card');
      card.dataset.id = prov.id;

      var name = el('div', 'provider-name', prov.name);
      card.appendChild(name);

      if (prov.model) {
        var model = el('div', 'provider-model', prov.model);
        card.appendChild(model);
      }

      card.addEventListener('click', function () {
        selectProvider(picker, prov, wrapper, traceId);
      });

      grid.appendChild(card);
    });

    picker.appendChild(grid);

    var noKey = el('div', 'no-key-hint');
    var link = el('a', null, 'get one free from Groq');
    link.href = 'https://console.groq.com/keys';
    link.target = '_blank';
    link.rel = 'noopener';
    noKey.appendChild(document.createTextNode("Don't have an API key? "));
    noKey.appendChild(link);
    picker.appendChild(noKey);

    wrapper.appendChild(picker);
  }

  function selectProvider(picker, provider, wrapper, traceId) {
    var existing = picker.querySelector('.provider-form');
    if (existing) existing.remove();

    Array.from(picker.querySelectorAll('.provider-card')).forEach(function (c) {
      c.classList.toggle('selected', c.dataset.id === provider.id);
    });

    var form = el('div', 'provider-form');

    if (provider.needsKey) {
      var keyLabel = el('label', 'form-label', 'API key');
      var keyInput = el('input', 'form-input');
      keyInput.type = 'password';
      keyInput.placeholder = 'sk-...';
      keyInput.autocomplete = 'off';
      form.appendChild(keyLabel);
      form.appendChild(keyInput);
    }

    if (provider.id === 'custom') {
      var urlLabel = el('label', 'form-label', 'Base URL');
      var urlInput = el('input', 'form-input');
      urlInput.type = 'text';
      urlInput.placeholder = 'https://api.example.com/v1';
      form.appendChild(urlLabel);
      form.appendChild(urlInput);

      var modelLabel = el('label', 'form-label', 'Model');
      var modelInput = el('input', 'form-input');
      modelInput.type = 'text';
      modelInput.placeholder = 'model-name';
      form.appendChild(modelLabel);
      form.appendChild(modelInput);
    }

    var actions = el('div', 'form-actions');
    var saveBtn = el('button', 'btn form-save-btn', 'save & explain');
    saveBtn.addEventListener('click', function () {
      var config = {
        providerId: provider.id,
        baseUrl: provider.baseUrl,
        model: provider.model,
        apiKey: '',
      };

      if (provider.needsKey) {
        var key = form.querySelector('input[type="password"]');
        if (!key || !key.value.trim()) {
          key.classList.add('input-error');
          return;
        }
        config.apiKey = key.value.trim();
      }

      if (provider.id === 'custom') {
        var inputs = form.querySelectorAll('input[type="text"]');
        if (!inputs[0].value.trim() || !inputs[1].value.trim()) {
          inputs[0].classList.toggle('input-error', !inputs[0].value.trim());
          inputs[1].classList.toggle('input-error', !inputs[1].value.trim());
          return;
        }
        config.baseUrl = inputs[0].value.trim().replace(/\/$/, '');
        config.model = inputs[1].value.trim();
      }

      saveConfig(config);
      picker.remove();
      runExplain(wrapper, traceId, config);
    });
    actions.appendChild(saveBtn);

    var cancelBtn = el('button', 'btn form-cancel-btn', 'cancel');
    cancelBtn.addEventListener('click', function () {
      picker.remove();
    });
    actions.appendChild(cancelBtn);

    form.appendChild(actions);
    picker.appendChild(form);

    var firstInput = form.querySelector('input');
    if (firstInput) firstInput.focus();
  }

  async function runExplain(wrapper, traceId, config, customPrompt) {
    var existing = wrapper.querySelector('.explain-result');
    if (existing) existing.remove();
    var existingPicker = wrapper.querySelector('.explain-picker');
    if (existingPicker) existingPicker.remove();

    var result = el('div', 'explain-result');
    result.innerHTML = '<div class="explain-loading">analyzing trace…</div>';
    wrapper.appendChild(result);

    var btn = wrapper.querySelector('.explain-btn');
    if (btn) { btn.disabled = true; btn.textContent = 'explaining…'; }

    try {
      var resp = await fetch('/api/traces/' + encodeURIComponent(traceId) + '/explain', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          api_key: config.apiKey || undefined,
          base_url: config.baseUrl || undefined,
          model: config.model || undefined,
          prompt: customPrompt || undefined,
        }),
      });

      if (!resp.ok) {
        var body = await resp.json().catch(function () { return {}; });
        var errMsg = body.error || resp.statusText;
        if (resp.status === 400) {
          clearConfig();
        }
        throw new Error(errMsg);
      }

      var data = await resp.json();
      result.innerHTML = '';

      var text = el('div', 'explain-text');
      text.innerHTML = formatExplanation(data.explanation);
      result.appendChild(text);

      var explainActions = el('div', 'explain-actions');
      var copyBtn = el('button', 'btn', 'copy');
      copyBtn.addEventListener('click', function () {
        navigator.clipboard.writeText(data.explanation).then(function () {
          copyBtn.textContent = 'copied';
          setTimeout(function () { copyBtn.textContent = 'copy'; }, 1500);
        }).catch(function () {});
      });
      explainActions.appendChild(copyBtn);

      var rerunBtn = el('button', 'btn', 'run again');
      rerunBtn.addEventListener('click', function () {
        var pi = wrapper.querySelector('.explain-prompt');
        runExplain(wrapper, traceId, loadConfig() || config, pi ? pi.value.trim() : '');
      });
      explainActions.appendChild(rerunBtn);

      result.appendChild(explainActions);
    } catch (e) {
      result.innerHTML = '';
      var errEl = el('div', 'explain-error');
      errEl.textContent = 'Explain failed: ' + e.message;
      result.appendChild(errEl);

      var retryActions = el('div', 'explain-actions');
      var settingsBtn = el('button', 'btn', 'change settings');
      settingsBtn.addEventListener('click', function () {
        result.remove();
        showProviderPicker(wrapper, traceId);
      });
      retryActions.appendChild(settingsBtn);
      result.appendChild(retryActions);
    } finally {
      if (btn) { btn.disabled = false; btn.textContent = 'explain'; }
    }
  }

  function formatExplanation(text) {
    return text
      .split(/\n\n+/)
      .map(function (p) {
        var escaped = p
          .replace(/&/g, '&amp;')
          .replace(/</g, '&lt;')
          .replace(/>/g, '&gt;')
          .replace(/`([^`]+)`/g, '<code>$1</code>')
          .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
        return '<p>' + escaped + '</p>';
      })
      .join('');
  }

  function el(tag, cls, text) {
    return window.el(tag, cls, text);
  }
})();
