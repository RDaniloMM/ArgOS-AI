import { useEffect, useState, useCallback } from "react";
import { Sparkles } from "lucide-react";
import { ProviderGrid } from "./components/ProviderGrid";
import { ProviderForm } from "./components/ProviderForm";
import { ToastContainer, type ToastMessage } from "./components/Toast";
import {
  getProviderPresets,
  getCurrentProvider,
  saveProvider,
  testProvider,
} from "./lib/tauri";
import type { ProviderPreset, ProviderInput } from "./types/provider";

export default function App() {
  const [presets, setPresets] = useState<ProviderPreset[]>([]);
  const [selectedPreset, setSelectedPreset] = useState<ProviderPreset | null>(null);
  const [currentInput, setCurrentInput] = useState<ProviderInput | null>(null);
  const [loading, setLoading] = useState(true);
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  useEffect(() => {
    async function load() {
      try {
        const [presetList, saved] = await Promise.all([
          getProviderPresets(),
          getCurrentProvider(),
        ]);
        setPresets(presetList);
        setCurrentInput(saved);
        if (saved) {
          const match = presetList.find((p) => p.id === saved.presetId);
          if (match) setSelectedPreset(match);
        }
      } catch (err) {
        addToast("error", "Failed to load settings", String(err));
      } finally {
        setLoading(false);
      }
    }
    load();
  }, []);

  const addToast = useCallback((type: "success" | "error", title: string, message: string) => {
    const id = `${Date.now()}-${Math.random()}`;
    setToasts((prev) => [...prev, { id, type, title, message }]);
  }, []);

  const dismissToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const handleSelectPreset = useCallback((preset: ProviderPreset) => {
    setSelectedPreset(preset);
  }, []);

  const handleSave = useCallback(
    async (input: ProviderInput) => {
      await saveProvider(input);
      setCurrentInput(input);
    },
    []
  );

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-slate-600 border-t-blue-500" />
      </div>
    );
  }

  return (
    <div className="min-h-full bg-gradient-to-br from-slate-900 via-slate-900 to-slate-800 px-6 py-10">
      <div className="mx-auto max-w-5xl">
        <header className="mb-10 text-center">
          <div className="mb-4 inline-flex items-center justify-center rounded-2xl bg-gradient-to-br from-blue-600 to-indigo-600 p-3 shadow-lg shadow-blue-600/20">
            <Sparkles className="h-7 w-7 text-white" />
          </div>
          <h1 className="text-3xl font-bold tracking-tight text-slate-100 sm:text-4xl">
            Provider Settings
          </h1>
          <p className="mx-auto mt-3 max-w-xl text-slate-400">
            Choose the LLM provider that powers ArgOS. Your API key is stored
            securely and never written to disk.
          </p>
        </header>

        <ProviderGrid
          presets={presets}
          selectedId={selectedPreset?.id ?? null}
          onSelect={handleSelectPreset}
        />

        {selectedPreset && (
          <div className="mt-8">
            <ProviderForm
              preset={selectedPreset}
              initialInput={currentInput}
              onTest={testProvider}
              onSave={handleSave}
              onToast={addToast}
            />
          </div>
        )}
      </div>

      <ToastContainer toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
