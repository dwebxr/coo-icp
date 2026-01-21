import { useState, useEffect, useRef } from 'react';
import { Actor, HttpAgent, Identity } from '@dfinity/agent';
import { AuthClient } from '@dfinity/auth-client';
import { idlFactory, canisterId, type _SERVICE, type Message } from './declarations/eliza_backend';

// Environment helpers
const getEnv = (key: string): string | undefined => {
  return (import.meta as any).env?.[key];
};

// Detect mainnet by checking hostname (icp0.io or ic0.app are mainnet domains)
const isMainnet = typeof window !== 'undefined' &&
  (window.location.hostname.endsWith('.icp0.io') ||
   window.location.hostname.endsWith('.ic0.app') ||
   window.location.hostname.endsWith('.raw.ic0.app'));

const iiCanisterId = getEnv('CANISTER_ID_INTERNET_IDENTITY') || 'rdmx6-jaaaa-aaaaa-aaadq-cai';

function App() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [actor, setActor] = useState<_SERVICE | null>(null);
  const [isAuthenticated, setIsAuthenticated] = useState(false);
  const [authClient, setAuthClient] = useState<AuthClient | null>(null);
  const [error, setError] = useState<string | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    initAuth();
  }, []);

  useEffect(() => {
    scrollToBottom();
  }, [messages, loading]);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  const initAuth = async () => {
    try {
      const client = await AuthClient.create();
      setAuthClient(client);

      if (await client.isAuthenticated()) {
        await setupActor(client.getIdentity());
        setIsAuthenticated(true);
      }
    } catch (err) {
      console.error('Auth init error:', err);
      setError('Failed to initialize authentication');
    }
  };

  const login = async () => {
    if (!authClient) return;

    try {
      const identityProvider = isMainnet
        ? 'https://identity.ic0.app'
        : `http://${iiCanisterId}.localhost:8080`;

      await authClient.login({
        identityProvider,
        onSuccess: async () => {
          await setupActor(authClient.getIdentity());
          setIsAuthenticated(true);
        },
        onError: (err) => {
          console.error('Login error:', err);
          setError('Login failed. Please try again.');
        },
      });
    } catch (err) {
      console.error('Login error:', err);
      setError('Login failed. Please try again.');
    }
  };

  const setupActor = async (identity: Identity) => {
    try {
      const agent = HttpAgent.createSync({
        identity,
        host: isMainnet ? 'https://icp-api.io' : undefined,
      });

      // Fetch root key for local development only
      if (!isMainnet) {
        await agent.fetchRootKey();
      }

      const elizaActor = Actor.createActor<_SERVICE>(idlFactory, {
        agent,
        canisterId: canisterId,
      });

      setActor(elizaActor);

      // Load conversation history
      const history = await elizaActor.get_conversation_history();
      const filteredHistory = history.filter((m: Message) => m.role !== 'system');
      setMessages(filteredHistory);
      setError(null);
    } catch (err) {
      console.error('Actor setup error:', err);
      setError('Failed to connect to the canister');
    }
  };

  const sendMessage = async () => {
    if (!input.trim() || !actor || loading) return;

    const userMessage = input.trim();
    setInput('');
    setLoading(true);
    setError(null);

    // Optimistically add user message to UI
    setMessages(prev => [...prev, { role: 'user', content: userMessage }]);

    try {
      const result = await actor.chat(userMessage);

      if ('Ok' in result) {
        setMessages(prev => [...prev, { role: 'assistant', content: result.Ok }]);
      } else {
        setError(`Error: ${result.Err}`);
        // Remove the optimistic user message on error
        setMessages(prev => prev.slice(0, -1));
      }
    } catch (err) {
      console.error('Chat error:', err);
      setError('Failed to send message. Please try again.');
      setMessages(prev => prev.slice(0, -1));
    } finally {
      setLoading(false);
    }
  };

  const clearConversation = async () => {
    if (!actor) return;

    try {
      await actor.clear_conversation();
      setMessages([]);
      setError(null);
    } catch (err) {
      console.error('Clear error:', err);
      setError('Failed to clear conversation');
    }
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  return (
    <div className="app">
      <header>
        <h1>Coo</h1>
        <p>Fully On-Chain AI Agent</p>
        <span className="subtitle">Powered by elizaOS framework on Internet Computer</span>
        <div className="status">
          <span className="status-dot"></span>
          Running on-chain
        </div>
      </header>

      {!isAuthenticated ? (
        <div className="login-container">
          <p>Connect with Internet Identity to start chatting with Coo</p>
          <button onClick={login} className="login-btn">
            Login with Internet Identity
          </button>
          {error && <p className="error">{error}</p>}
        </div>
      ) : (
        <div className="chat-container">
          <div className="chat-header">
            <h2>Chat with Coo</h2>
            <button onClick={clearConversation} className="clear-btn">
              Clear Chat
            </button>
          </div>

          <div className="messages">
            {messages.length === 0 && !loading && (
              <div className="welcome">
                <h3>Welcome!</h3>
                <p>Start a conversation with Coo, your on-chain AI assistant built on elizaOS.</p>
              </div>
            )}

            {messages.map((msg, i) => (
              <div key={i} className={`message ${msg.role}`}>
                <strong>{msg.role === 'user' ? 'You' : 'Coo'}</strong>
                <p>{msg.content}</p>
              </div>
            ))}

            {loading && (
              <div className="loading">
                <span>Coo is thinking</span>
                <div className="loading-dots">
                  <span></span>
                  <span></span>
                  <span></span>
                </div>
              </div>
            )}

            {error && <p className="error">{error}</p>}
            <div ref={messagesEndRef} />
          </div>

          <div className="input-area">
            <input
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyPress={handleKeyPress}
              placeholder="Message Coo..."
              disabled={loading}
            />
            <button onClick={sendMessage} disabled={loading || !input.trim()}>
              Send
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
