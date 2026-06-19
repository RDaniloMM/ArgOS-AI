import { iconById } from "./Icon";
import type { ProviderPreset } from "../types/provider";

interface ProviderGridProps {
  presets: ProviderPreset[];
  selectedId: string | null;
  onSelect: (preset: ProviderPreset) => void;
}

export function ProviderGrid({ presets, selectedId, onSelect }: ProviderGridProps) {
  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {presets.map((preset) => {
        const Icon = iconById[preset.icon];
        const isSelected = selectedId === preset.id;
        return (
          <button
            key={preset.id}
            type="button"
            onClick={() => onSelect(preset)}
            className={[
              "group relative flex flex-col items-start rounded-2xl border p-5 text-left transition-all duration-200",
              "border-slate-700/50 bg-gradient-to-br from-slate-800/80 to-slate-900/80 hover:border-slate-600 hover:from-slate-800 hover:to-slate-900 hover:shadow-lg",
              isSelected
                ? "provider-card-glow border-blue-500/80 from-slate-800 to-blue-950/30"
                : "",
            ].join(" ")}
          >
            <div
              className={[
                "mb-4 flex h-12 w-12 items-center justify-center rounded-xl text-slate-100 transition-colors",
                isSelected
                  ? "bg-blue-600 shadow-lg shadow-blue-600/25"
                  : "bg-slate-700/60 group-hover:bg-slate-700",
              ].join(" ")}
            >
              {Icon ? <Icon className="h-6 w-6" /> : null}
            </div>
            <h3 className="text-base font-semibold text-slate-100">{preset.name}</h3>
            <p className="mt-1 text-sm leading-relaxed text-slate-400">
              {preset.description}
            </p>
            <span className="mt-3 inline-flex items-center text-xs font-medium text-blue-400">
              {preset.defaultModel}
            </span>
          </button>
        );
      })}
    </div>
  );
}
