import { useState, useEffect, useRef, useCallback } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { useStore } from 'zustand'
import type { ApplicationRenderProps, ChatState, ChatMessage } from '../types'
import { useSettingsStore } from '../store/settings'
import { useAppStore } from '../store/app'

export default function Chat({ tab, workspace, isVisible }: ApplicationRenderProps) {
  const { settings } = useSettingsStore()
  const llm = useAppStore((s) => s.llm)
  const state = tab.state as ChatState
  const [messages, setMessages] = useState<ChatMessage[]>(state.messages)
  const [input, setInput] = useState('')
  const [isStreaming, setIsStreaming] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [reasoning, setReasoning] = useState(false)
  const activeRequestId = useRef<string | null>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const workspaceStore = useStore(workspace)

  // Scroll to bottom when messages change
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  // Persist messages to tab state
  useEffect(() => {
    workspaceStore.updateTabState<ChatState>(tab.id, () => ({ messages }))
  }, [messages])

  // Subscribe to LLM events
  useEffect(() => {
    const unsubDelta = llm.onDelta((requestId, text) => {
      if (requestId !== activeRequestId.current) return
      setMessages((prev) => {
        const last = prev[prev.length - 1]
        if (last && last.role === 'assistant') {
          return [...prev.slice(0, -1), { ...last, content: last.content + text }]
        }
        return prev
      })
    })

    const unsubDone = llm.onDone((requestId) => {
      if (requestId !== activeRequestId.current) return
      setIsStreaming(false)
      activeRequestId.current = null
    })

    const unsubError = llm.onError((requestId, errorMsg) => {
      if (requestId !== activeRequestId.current) return
      setError(errorMsg)
      setIsStreaming(false)
      activeRequestId.current = null
    })

    return () => {
      unsubDelta()
      unsubDone()
      unsubError()
    }
  }, [llm])

  const handleSend = useCallback(async () => {
    const trimmed = input.trim()
    if (!trimmed || isStreaming) return

    if (!settings.llm.apiKey) {
      setError('LLM API key not configured. Go to Settings > LLM to set it up.')
      return
    }

    setError(null)
    const userMsg: ChatMessage = {
      id: crypto.randomUUID(),
      role: 'user',
      content: trimmed
    }
    const assistantMsg: ChatMessage = {
      id: crypto.randomUUID(),
      role: 'assistant',
      content: ''
    }

    const updated = [...messages, userMsg, assistantMsg]
    setMessages(updated)
    setInput('')
    setIsStreaming(true)

    const requestId = crypto.randomUUID()
    activeRequestId.current = requestId

    // Build messages for the API (user + assistant history, skip empty assistant)
    const apiMessages = updated
      .filter((m) => m.content.length > 0)
      .map((m) => ({ role: m.role, content: m.content }))

    try {
      await llm.send(requestId, apiMessages, {
        baseUrl: settings.llm.baseUrl,
        apiKey: settings.llm.apiKey,
        model: settings.llm.model,
        reasoning
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to send message')
      setIsStreaming(false)
      activeRequestId.current = null
    }
  }, [input, messages, isStreaming, settings.llm, llm, reasoning])

  const handleCancel = useCallback(() => {
    if (activeRequestId.current) {
      llm.cancel(activeRequestId.current)
      setIsStreaming(false)
      activeRequestId.current = null
    }
  }, [llm])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault()
        handleSend()
      }
    },
    [handleSend]
  )

  if (!isVisible) return null

  return (
    <div className="chat-container">
      <div className="chat-messages">
        {messages.length === 0 && (
          <div className="chat-empty">
            <p>Start a conversation</p>
            <p className="chat-empty-hint">
              Using {settings.llm.model} at {settings.llm.baseUrl}
            </p>
          </div>
        )}
        {messages.map((msg) => (
          <div key={msg.id} className={`chat-message chat-message-${msg.role}`}>
            <div className="chat-message-role">{msg.role === 'user' ? 'You' : 'Assistant'}</div>
            <div className="chat-message-content">
              {msg.role === 'assistant' ? (
                msg.content ? (
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content}</ReactMarkdown>
                ) : (
                  isStreaming && <span className="chat-typing">Thinking...</span>
                )
              ) : (
                <p>{msg.content}</p>
              )}
            </div>
          </div>
        ))}
        {error && <div className="chat-error">{error}</div>}
        <div ref={messagesEndRef} />
      </div>
      <div className="chat-input-area">
        <div className="chat-input-options">
          <label className="chat-reasoning-label">
            <input
              type="checkbox"
              checked={reasoning}
              onChange={(e) => setReasoning(e.target.checked)}
              disabled={isStreaming}
            />
            Reasoning
          </label>
        </div>
        <textarea
          className="chat-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type a message... (Enter to send, Shift+Enter for newline)"
          disabled={isStreaming}
          rows={3}
        />
        <div className="chat-input-actions">
          {isStreaming ? (
            <button className="chat-btn chat-btn-cancel" onClick={handleCancel}>
              Cancel
            </button>
          ) : (
            <button
              className="chat-btn chat-btn-send"
              onClick={handleSend}
              disabled={!input.trim()}
            >
              Send
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
