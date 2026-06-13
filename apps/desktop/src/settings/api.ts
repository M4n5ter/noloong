import { invoke } from "@tauri-apps/api/core";
import {
  devProfileConfigCompletions,
  devProfileConfigDocument,
  devValidateProfileConfig,
  isTauriRuntime,
} from "../devFallback";
import type {
  AppProfileConfigCompletionSet,
  AppProfileConfigDocument,
  AppProfileConfigValidationResult,
  AppRuntimeRestartResult,
} from "../generated/contracts";

export function loadProfileConfig(): Promise<AppProfileConfigDocument> {
  if (!isTauriRuntime()) {
    return Promise.resolve(devProfileConfigDocument());
  }
  return invoke("app_profile_config_load");
}

export function validateProfileConfig(text: string): Promise<AppProfileConfigValidationResult> {
  if (!isTauriRuntime()) {
    return Promise.resolve(devValidateProfileConfig(text));
  }
  return invoke("app_profile_config_validate", { request: { text } });
}

export function saveProfileConfig(text: string): Promise<AppProfileConfigDocument> {
  if (!isTauriRuntime()) {
    return Promise.resolve(devProfileConfigDocument(text));
  }
  return invoke("app_profile_config_save", { request: { text } });
}

export function restartInteraction(): Promise<AppRuntimeRestartResult> {
  return invoke("app_runtime_restart_interaction");
}

export function completeProfileConfig(
  text: string,
  offset: number,
): Promise<AppProfileConfigCompletionSet> {
  if (!isTauriRuntime()) {
    return Promise.resolve(devProfileConfigCompletions());
  }
  return invoke("app_profile_config_completions", { request: { text, offset } });
}
