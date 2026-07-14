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
  requestLifecycleRematch,
  respondLifecycleRematch,
  expireLifecycleRematch,
  denyLifecycleRematch,
  leaveLifecycleMatch,
  requeueLifecyclePlayer,
  markLifecycleActiveReconnect,
  rolloverLifecycleActiveReconnect,
} from "../vendor/cloudflare-game-common/lifecycle.js";

export const createEpochState = createLifecycleState;
export const canonicalReport = canonicalOutcomes;
export const selectNextRoster = selectLifecycleRoster;
export const startNextEpoch = startLifecycleRound;
export const submitReport = submitLifecycleReport;
export const abort = abortLifecycleRound;
export const validateSignal = validateEpochSignal;
export const requestRematch = requestLifecycleRematch;
export const respondRematch = respondLifecycleRematch;
export const expireRematch = expireLifecycleRematch;
export const denyRematch = denyLifecycleRematch;
export const leaveMatch = leaveLifecycleMatch;
export const requeuePlayer = requeueLifecyclePlayer;
export const markActiveReconnect = markLifecycleActiveReconnect;
export const rolloverActiveReconnect = rolloverLifecycleActiveReconnect;
