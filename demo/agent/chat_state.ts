/** 对话状态：流式内容与工具状态分开保存。 */
export interface ChatState {
  text: string;
  status: 'idle' | 'streaming' | 'done' | 'error';
  error: string | null;
}

export function appendToken(state: ChatState, token: string): ChatState {
  return { ...state, text: state.text + token, status: 'streaming' };
}

export function finishReply(state: ChatState): ChatState {
  return { ...state, status: 'done' };
}

export function failReply(state: ChatState, error: unknown): ChatState {
  const message = error instanceof Error ? error.message : String(error);
  return { ...state, status: 'error', error: message };
}

