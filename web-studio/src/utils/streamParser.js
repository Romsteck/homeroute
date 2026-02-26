/**
 * Walk backward through messages to find the last tool_use or ask_user_question
 * without a status and annotate it with 'success' or 'error'.
 * Returns { type, hidden } so callers can decide if tool_result should be hidden.
 */
function annotateLastToolUse(messages, isError) {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if ((m.type === 'tool_use' || m.type === 'ask_user_question') && !m.status) {
      messages[i] = { ...m, status: isError ? 'error' : 'success' };
      return { type: m.type, hidden: m.hidden || false };
    }
  }
  return null;
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
          // AskUserQuestion gets a special message type for interactive rendering
          if (block.name === 'AskUserQuestion') {
            const questions = block.input?.questions || [];
            // Dedup: skip if previous message has identical questions
            const prev = next[next.length - 1];
            if (prev?.type === 'ask_user_question' &&
                JSON.stringify(prev.questions) === JSON.stringify(questions)) {
              // Update tool_use_id for annotation tracking
              next[next.length - 1] = { ...prev, tool_use_id: block.id };
            } else {
              next.push({
                type: 'ask_user_question',
                questions,
                tool_use_id: block.id,
              });
            }
          } else if (block.name === 'TodoWrite') {
            // TodoWrite is hidden — TodoPanel already displays todos
            next.push({
              type: 'tool_use',
              tool: 'TodoWrite',
              input: block.input,
              tool_use_id: block.id,
              hidden: true,
            });
          } else if (block.name === 'EnterPlanMode' || block.name === 'ExitPlanMode') {
            // Plan mode tools are hidden — PlanComplete UI handles the visual feedback
            next.push({
              type: 'tool_use',
              tool: block.name,
              input: block.input,
              tool_use_id: block.id,
              hidden: true,
            });
          } else {
            next.push({
              type: 'tool_use',
              tool: block.name || 'unknown',
              input: block.input,
              tool_use_id: block.id,
            });
          }
        } else if (block.type === 'tool_result') {
          const text = Array.isArray(block.content)
            ? block.content.map(c => c.text || '').join('\n')
            : (typeof block.content === 'string' ? block.content : '');
          const annotated = annotateLastToolUse(next, block.is_error || false);
          next.push({
            type: 'tool_result',
            content: text,
            is_error: block.is_error || false,
            hidden: annotated?.type === 'ask_user_question' || annotated?.hidden || false,
          });
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
    const annotated = annotateLastToolUse(next, event.is_error || false);
    next.push({
      type: 'tool_result',
      content: text,
      is_error: event.is_error || false,
      hidden: annotated?.type === 'ask_user_question' || annotated?.hidden || false,
    });
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
