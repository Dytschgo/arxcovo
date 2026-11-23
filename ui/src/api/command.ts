import type { TerminalCommand } from './types';

export const MAX_ARGS = 128;
export const MAX_ARG_LENGTH = 8192;

export type ParsedCommand = { ok: true; command: TerminalCommand } | { ok: false; error: string };

/**
 * Splits a command line into a tool and arguments. Supports single and double
 * quotes and backslash escapes inside double quotes. No shell: no globbing,
 * pipes, variables or redirection, matching the bounded run_command executor.
 */
export function parseCommandLine(line: string): ParsedCommand {
  const tokens: string[] = [];
  let current = '';
  let inToken = false;
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i];
    if (quote) {
      if (c === quote) quote = null;
      else if (c === '\\' && quote === '"' && (line[i + 1] === '"' || line[i + 1] === '\\')) current += line[++i];
      else current += c;
    } else if (c === '"' || c === "'") {
      quote = c;
      inToken = true;
    } else if (/\s/.test(c)) {
      if (inToken) tokens.push(current);
      current = '';
      inToken = false;
    } else {
      current += c;
      inToken = true;
    }
  }
  if (quote) return { ok: false, error: 'Close the open quote.' };
  if (inToken) tokens.push(current);
  if (tokens.length === 0) return { ok: false, error: 'Type a tool name, for example: git --version' };
  const [tool, ...args] = tokens;
  if (!/^[A-Za-z0-9][A-Za-z0-9._+-]{0,63}$/.test(tool)) return { ok: false, error: 'The tool name must be a plain program name, not a path.' };
  if (args.length > MAX_ARGS) return { ok: false, error: `At most ${MAX_ARGS} arguments.` };
  if (args.some(a => a.length > MAX_ARG_LENGTH)) return { ok: false, error: `Each argument is limited to ${MAX_ARG_LENGTH} characters.` };
  return { ok: true, command: { tool, args } };
}
