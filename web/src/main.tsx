import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { claimToken, noteUnauthorized, streamUrl, withToken } from './lib/play-token'
import { isNetworkFailure, noteReachable, noteUnreachable } from './lib/server-reach'

const token = claimToken(sessionStorage, window.location, url => {
  window.history.replaceState(null, '', url)
})

const send = window.fetch.bind(window)
window.fetch = async (input, init) => {
  try {
    const answer = await send(input, token === null ? init : withToken(token, init))
    noteReachable()
    if (answer.status === 401) noteUnauthorized()
    return answer
  } catch (err) {
    if (isNetworkFailure(err)) noteUnreachable()
    throw err
  }
}

if (token !== null) {
  const Source = window.EventSource
  window.EventSource = class extends Source {
    constructor(url: string | URL, init?: EventSourceInit) {
      super(streamUrl(token, String(url)), init)
    }
  } as typeof EventSource
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
