import type { Backend } from './backend';
import { Chat } from './components/Chat';
import { SignIn } from './components/SignIn';
import { useCoo } from './useCoo';

function App({ backend }: { backend: Backend }) {
  const coo = useCoo(backend);

  if (coo.status === 'ready') return <Chat coo={coo} />;

  return (
    <SignIn
      checking={coo.status === 'checking'}
      signingIn={coo.status === 'signingIn'}
      ready={coo.authReady}
      notice={coo.notice}
      onSignIn={coo.signIn}
      onCancel={coo.cancelSignIn}
    />
  );
}

export default App;
