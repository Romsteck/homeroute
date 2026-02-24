/**
 * Walk backward through messages to find the last tool_use without a status
 * and annotate it with 'success' or 'error' based on the tool_result.
 */
function annotateLastToolUse(messages, isError) {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].type === 'tool_use' && !messages[i].status) {
      messages[i] = { ...messages[i], status: isError ? 'error' : 'success' };
      break;
    }
  }
}

/**
 * Process a Claude Code stream-json event and update the messages array.
 *
 * Claude stream-json format:
 * - Init:      {"subtype":"init","session_id":"...","tools":[...],...}
 * - Assistant:  {"type":"assistant","message":{"content":[{"type":"text","text":"..."},{"type":"tool_use","name":"Read","input":{...}}]},...}
 * - Result:     {"type":"result","subtype":"success","result":"...","session_id":"...","is_error":false,...}
 */
export function updateMessagesFromStream(messages, event) {
  const next = [...messages];

  // Init event — skip (system metadata)
  if (event.subtype === 'init' && !event.type) {
    return next;
  }
  if (event.type === 'system') {
    return next;
  }

  // Assistant message — extract content blocks
  if (event.type === 'assistant') {
    const contentBlocks = event.message?.content;
    if (Array.isArray(contentBlocks)) {
      for (const block of contentBlocks) {
        if (block.type === 'text' && block.text) {
          // Append to last assistant text message or create new one
          const last = next[next.length - 1];
          if (last && last.type === 'assistant' && last.subtype === 'text' && !last.complete) {
            next[next.length - 1] = {
              ...last,
              content: last.content + block.text,
            };
          } else {
            next.push({
              type: 'assistant',
              subtype: 'text',
              content: block.text,
              complete: false,
            });
          }
        } else if (block.type === 'tool_use') {
          // Mark previous assistant text as complete
          const last = next[next.length - 1];
          if (last && last.type === 'assistant' && last.subtype === 'text') {
            next[next.length - 1] = { ...last, complete: true };
          }
          next.push({
            type: 'tool_use',
            tool: block.name || 'unknown',
            input: block.input,
            tool_use_id: block.id,
          });
        } else if (block.type === 'tool_result') {
          const text = Array.isArray(block.content)
            ? block.content.map(c => c.text || '').join('\n')
            : (typeof block.content === 'string' ? block.content : '');
          next.push({
            type: 'tool_result',
            content: text,
            is_error: block.is_error || false,
          });
          annotateLastToolUse(next, block.is_error || false);
        }
      }
    }
    // Mark the last assistant text as complete when the message has stop_reason
    if (event.stop_reason) {
      const last = next[next.length - 1];
      if (last && last.type === 'assistant' && last.subtype === 'text') {
        next[next.length - 1] = { ...last, complete: true };
      }
    }
    return next;
  }

  // Tool result (standalone, not nested in assistant content)
  if (event.type === 'tool_result' || event.subtype === 'tool_result') {
    const text = Array.isArray(event.content)
      ? event.content.map(c => c.text || '').join('\n')
      : (typeof event.content === 'string' ? event.content : JSON.stringify(event.content || ''));
    next.push({
      type: 'tool_result',
      content: text,
      is_error: event.is_error || false,
    });
    annotateLastToolUse(next, event.is_error || false);
    return next;
  }

  // Result event — final response
  if (event.type === 'result') {
    if (event.is_error) {
      next.push({
        type: 'error',
        content: event.error || event.result || 'An error occurred',
      });
    }
    // Don't add a message for successful results — the content was already streamed
    return next;
  }

  // Unknown event — skip silently (don't show raw JSON)
  return next;
}
