import { z } from 'zod'
import { ActivityState, ClassifierProvider, type ClassifiedState, type ClassifierCriteria } from './types'

/** Order in which criteria are listed in the settings page and the generated chat prompt. */
export const classifiedStates = [
  ActivityState.Working,
  ActivityState.SafePermissionRequested,
  ActivityState.PermissionRequest,
  ActivityState.Completed,
  ActivityState.UserInputRequired,
  ActivityState.ApplicationError,
  ActivityState.Idle,
] as const satisfies readonly ClassifiedState[]

export const defaultClassifierCriteria: ClassifierCriteria = {
  [ActivityState.Working]: 'Program is still running and producing output; nothing is waiting on the user.',
  [ActivityState.SafePermissionRequested]: 'Program asks for permission, but the action is safe. A safe action is one of:\n  - A git operation, unless it changes another worktree.\n  - Build, test, or installing dependencies.\n  - Anything that only mutates files within one of the safe paths ({{safe_paths}}).\n  Reading outside the safe paths is allowed, just not mutating them. A wide-ranging search via find or `grep -r` outside the safe paths is not safe.',
  [ActivityState.PermissionRequest]: 'Program asks for y/n or similar confirmation, but the action does not meet the safe_permission_requested criteria.',
  [ActivityState.Completed]: 'The previous user request has been satisfied.',
  [ActivityState.UserInputRequired]: 'Program asks for user text input, a choice among design options, or a plan confirmation.',
  [ActivityState.ApplicationError]: 'The program itself failed and stopped: service overloaded, rate limited, API or network error, crash, or unhandled exception. The user must retry. Not a failing command that the program ran and is still handling.',
  [ActivityState.Idle]: 'Shell prompt visible, waiting for a command, or user input is incomplete.',
}

/** Context and policy only; the state list and output format are generated from the criteria. */
export const defaultAnalyzerSystemPrompt = 'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, classify the terminal\'s activity state.'

/** Every analyzer prompt previously shipped as a default, which embedded the state list. A saved copy of one is replaced on load. */
export const legacyDefaultSystemPrompts: readonly string[] = [
  'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is one of the following that best represent the state: \n - "working" program is still running and producing output, nothing is waiting on the user.\n - "safe_permission_requested" program asking for permission but the action is safe. A safe action are action that either:\n   - Git operation unless it changes other worktree.\n   - Build or test or install dependencies.\n   - Anything that only mutates files within one of the safe paths.\n   - It IS allowed to read outside the safe path, just not mutate them. \n   - But it is not allowed to do a wide ranging search via find or `grep -r` on file outside the safe path.\n - "permission_request" program asking for y/n or similar confirmation but it is not considered safe as "safe_permission_requested".\n - "completed" when previous user request satisfied.\n - "user_input_required" program asking for user text input or confirmation among design choice or a plan confirmation.\n - "application_error" the program itself failed and stopped: service overloaded, rate limited, API or network error, crash, or unhandled exception. The user must retry. Not a failing command that the program ran and is still handling.\n - "idle" for shell prompt visible, waiting for command or user input is incomplete.\nThe reason field should show the reason for the verdict, no more than 10 word. ',
  'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is one of the following that best represent the state: \n - "working" program is still running and producing output, nothing is waiting on the user.\n - "safe_permission_requested" program asking for permission but the action is safe. A safe action are action that either:\n   - Git operation unless it changes other worktree.\n   - Build or test or install dependencies.\n   - Anything that only mutates files within one of the safe paths.\n   - It IS allowed to read outside the safe path, just not mutate them. \n   - But it is not allowed to do a wide ranging search via find or `grep -r` on file outside the safe path.\n - "permission_request" program asking for y/n or similar confirmation but it is not considered safe as "safe_permission_requested".\n - "completed" when previous user request satisfied.\n - "user_input_required" program asking for user text input or confirmation among design choice or a plan confirmation.\n - "idle" for shell prompt visible, waiting for command or user input is incomplete.\nThe reason field should show the reason for the verdict, no more than 10 word. ',
  'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is one of the following that best represent the state: \n - "safe_permission_requested" program asking for permission but the action is safe. A safe action are action that either:\n   - Git operation unless it changes other worktree.\n   - Build or test or install dependencies.\n   - Anything that only mutates files within one of the safe paths.\n   - It IS allowed to read outside the safe path, just not mutate them. \n   - But it is not allowed to do a wide ranging search via find or `grep -r` on file outside the safe path.\n - "permission_request" program asking for y/n or similar confirmation but it is not considered safe as "safe_permission_requested".\n - "completed" when previous user request satisfied.\n - "user_input_required" program asking for user text input or confirmation among design choice or a plan confirmation.\n - "idle" for shell prompt visible, waiting for command or user input is incomplete.\nThe reason field should show the reason for the verdict, no more than 10 word. ',
  'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is one of: "idle" (shell prompt visible, waiting for command), "working" (process actively running/producing output), "user_input_required" (program asking for user text input), "permission_request" (program asking for y/n or similar confirmation and the action may mutate files OUTSIDE the safe paths), "safe_permission_requested" (program asking for permission but the action only mutates files within one of the safe paths — safe to approve), "completed" (task finished, showing final result). "working" override other state and reason is the reason for the verdict, no more than 10 word. A git operation is considered safe as long as it does not mutate other branch. A plan approval is considered "user_input_required" even if it only mutate safe path.',
  'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where reason is a short explanation of why you chose this state, and state is one of: "idle" (shell prompt visible, waiting for command), "working" (process actively running/producing output), "user_input_required" (program asking for user text input), "permission_request" (program asking for y/n or similar confirmation and the action may mutate files OUTSIDE the safe paths), "safe_permission_requested" (program asking for permission but the action only mutates files within one of the safe paths — safe to approve), "completed" (task finished, showing final result). No other text.',
  'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>"} where state is one of: "idle" (shell prompt visible, waiting for command), "working" (process actively running/producing output), "user_input_required" (program asking for user text input), "permission_request" (program asking for y/n or similar confirmation and the action may mutate files OUTSIDE the safe paths), "safe_permission_requested" (program asking for permission but the action only mutates files within one of the safe paths — safe to approve), "completed" (task finished, showing final result). No other text.',
]

const criteriaSchema = z.object({
  [ActivityState.Working]: z.string(),
  [ActivityState.SafePermissionRequested]: z.string(),
  [ActivityState.PermissionRequest]: z.string(),
  [ActivityState.Completed]: z.string(),
  [ActivityState.UserInputRequired]: z.string(),
  [ActivityState.ApplicationError]: z.string(),
  [ActivityState.Idle]: z.string(),
}) satisfies z.ZodType<ClassifierCriteria>

/** Validate new classifier settings at the persistence boundary; empty models are UI configuration errors. */
export const classifierSettingsSchema = z.object({
  provider: z.enum(ClassifierProvider),
  model: z.string(),
  titleModel: z.string(),
  criteria: criteriaSchema,
})
