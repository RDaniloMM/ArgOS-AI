import { invoke } from "@tauri-apps/api/core";
import type { ProviderPreset, ProviderInput, ProviderStatus } from "../types/provider";

export async function getProviderPresets(): Promise<ProviderPreset[]> {
  return invoke<ProviderPreset[]>("get_provider_presets");
}

export async function getCurrentProvider(): Promise<ProviderInput | null> {
  return invoke<ProviderInput | null>("get_current_provider");
}

export async function saveProvider(input: ProviderInput): Promise<void> {
  return invoke("save_provider", { input });
}

export async function testProvider(input: ProviderInput): Promise<ProviderStatus> {
  return invoke<ProviderStatus>("test_provider", { input });
}
