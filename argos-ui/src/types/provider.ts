export interface ProviderPreset {
  id: string;
  name: string;
  description: string;
  defaultEndpoint: string;
  defaultModel: string;
  icon: string;
}

export interface ProviderInput {
  presetId: string;
  apiKey: string;
  endpoint: string;
  model: string;
}

export interface ProviderStatus {
  connected: boolean;
  message: string;
}
