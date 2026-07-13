// The lifecycle reducer is vendored so this Worker and car_game_ai can consume
// byte-for-byte compatible state-transition semantics without an npm runtime.
import {
  abortLifecycleRound,
  canonicalOutcomes,
  createLifecycleState,
  selectLifecycleRoster,
  startLifecycleRound,
  submitLifecycleReport,
  validateEpochSignal,
} from "../vendor/cloudflare-game-common/lifecycle.js";

export const createEpochState = createLifecycleState;
export const canonicalReport = canonicalOutcomes;
export const selectNextRoster = selectLifecycleRoster;
export const startNextEpoch = startLifecycleRound;
export const submitReport = submitLifecycleReport;
export const abort = abortLifecycleRound;
export const validateSignal = validateEpochSignal;
