export function updateMessagesFromStream(messages, event) {
  const next = [...messages];
  const { type, subtype } = event;

  if (type === 'system') {
    return next;
  }

  if (type === 'assistant' && subtype === 'text') {
    const last = next[next.length - 1];
    if (last && last.type === 'assistant' && last.subtype === 'text' && !last.complete) {
      next[next.length - 1] = {
        ...last,
        content: (last.content || '') + (event.content || event.text || ''),
      };
    } else {
      next.push({
        type: 'assistant',
        subtype: 'text',
        content: event.content || event.text || '',
      });
    }
    return next;
  }

  if (type === 'assistant' && subtype === 'tool_use') {
    next.push({
      type: 'tool_use',
      tool: event.tool_name || event.name || 'unknown',
      input: event.input,
    });
    return next;
  }

  if (subtype === 'tool_result' || type === 'tool_result') {
    next.push({
      type: 'tool_result',
      content: event.content || '',
      is_error: event.is_error || false,
    });
    return next;
  }

  if (type === 'result') {
    if (event.is_error) {
      next.push({
        type: 'error',
        content: event.error || event.content || 'An error occurred',
      });
    }
    return next;
  }

  next.push({ type: 'raw', data: event });
  return next;
}

export function parseSessionMessage(jsonlLine) {
  try {
    const event = typeof jsonlLine === 'string' ? JSON.parse(jsonlLine) : jsonlLine;
    return event;
  } catch {
    return null;
  }
}
