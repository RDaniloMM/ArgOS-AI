import { useState, useEffect } from "react";
import { TestTube2, Save, Loader2 } from "lucide-react";
import type { ProviderPreset, ProviderInput, ProviderStatus } from "../types/provider";

interface ProviderFormProps {
  preset: ProviderPreset;
  initialInput: ProviderInput | null;
  onTest: (input: ProviderInput) => Promise<ProviderStatus>;
  onSave: (input: ProviderInput) => Promise<void>;
  onToast: (type: "success" | "error", title: string, message: string) => void;
}

export function ProviderForm({
  preset,
  initialInput,
  onTest,
  onSave,
  onToast,
}: ProviderFormProps) {
  const [apiKey, setApiKey] = useState("");
  const [endpoint, setEndpoint] = useState(preset.defaultEndpoint);
  const [model, setModel] = useState(preset.defaultModel);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (initialInput && initialInput.presetId === preset.id) {
      setApiKey(initialInput.apiKey);
      setEndpoint(initialInput.endpoint || preset.defaultEndpoint);
      setModel(initialInput.model || preset.defaultModel);
    } else {
      setApiKey("");
      setEndpoint(preset.defaultEndpoint);
      setModel(preset.defaultModel);
    }
  }, [preset, initialInput]);

  const input: ProviderInput = {
    presetId: preset.id,
    apiKey,
    endpoint,
    model,
  };

  async function handleTest() {
    setTesting(true);
    try {
      const status = await onTest(input);
      if (status.connected) {
        onToast("success", "Connection successful", status.message);
      } else {
        onToast("error", "Connection failed", status.message);
      }
    } catch (err) {
      onToast("error", "Connection error", String(err));
    } finally {
      setTesting(false);
    }
  }

  async function handleSave() {
    setSaving(true);
    try {
      await onSave(input);
      onToast("success", "Provider saved", `${preset.name} configuration saved.`);
    } catch (err) {
      onToast("error", "Save failed", String(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="animate-fade rounded-2xl border border-slate-700/50 bg-slate-800/50 p-6 backdrop-blur">
      <h2 className="text-lg font-semibold text-slate-100">
        Configure {preset.name}
      </h2>
      <p className="mt-1 text-sm text-slate-400">
        Enter the details for your {preset.name} provider.
      </p>

      <div className="mt-6 grid gap-5">
        <div className="grid gap-2">
          <label htmlFor="endpoint" className="text-sm font-medium text-slate-300">
            Endpoint
          </label>
          <input
            id="endpoint"
            type="url"
            value={endpoint}
            onChange={(e) => setEndpoint(e.target.value)}
            placeholder={preset.defaultEndpoint}
            className="rounded-xl border border-slate-700 bg-slate-900/80 px-4 py-2.5 text-sm text-slate-100 placeholder:text-slate-600 focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
          />
          <p className="text-xs text-slate-500">
            Use the base URL. Full <code>/chat/completions</code> URLs are accepted and normalized automatically.
          </p>
        </div>

        <div className="grid gap-2">
          <label htmlFor="model" className="text-sm font-medium text-slate-300">
            Model
          </label>
          <input
            id="model"
            type="text"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder={preset.defaultModel}
            className="rounded-xl border border-slate-700 bg-slate-900/80 px-4 py-2.5 text-sm text-slate-100 placeholder:text-slate-600 focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
          />
        </div>

        <div className="grid gap-2">
          <label htmlFor="apiKey" className="text-sm font-medium text-slate-300">
            API Key
          </label>
          <input
            id="apiKey"
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="sk-..."
            className="rounded-xl border border-slate-700 bg-slate-900/80 px-4 py-2.5 text-sm text-slate-100 placeholder:text-slate-600 focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
          />
          <p className="text-xs text-slate-500">
            Stored securely in the system vault, never written to config.toml.
          </p>
        </div>
      </div>

      <div className="mt-8 flex flex-col-reverse gap-3 sm:flex-row sm:justify-end">
        <button
          type="button"
          onClick={handleTest}
          disabled={testing}
          className="inline-flex items-center justify-center gap-2 rounded-full border border-slate-600 bg-slate-800 px-5 py-2.5 text-sm font-medium text-slate-200 transition-colors hover:bg-slate-700 disabled:opacity-60"
        >
          {testing ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <TestTube2 className="h-4 w-4" />
          )}
          Test Connection
        </button>
        <button
          type="button"
          onClick={handleSave}
          disabled={saving}
          className="inline-flex items-center justify-center gap-2 rounded-full bg-gradient-to-r from-blue-600 to-indigo-600 px-6 py-2.5 text-sm font-semibold text-white shadow-lg shadow-blue-600/20 transition-transform hover:scale-[1.02] active:scale-[0.98] disabled:opacity-60"
        >
          {saving ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Save className="h-4 w-4" />
          )}
          Save Provider
        </button>
      </div>
    </div>
  );
}
