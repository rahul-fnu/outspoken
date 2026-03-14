import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function App() {
  const [greeting, setGreeting] = useState("");
  const [name, setName] = useState("");

  async function greet() {
    const message = await invoke<string>("greet", { name });
    setGreeting(message);
  }

  return (
    <main style={{ padding: "2rem", fontFamily: "sans-serif" }}>
      <h1>Outspoken</h1>
      <p>AI-powered dictation, right on your desktop.</p>

      <div style={{ marginTop: "1rem" }}>
        <input
          id="greet-input"
          onChange={(e) => setName(e.currentTarget.value)}
          placeholder="Enter a name..."
        />
        <button type="button" onClick={greet}>
          Greet
        </button>
      </div>

      <p>{greeting}</p>
    </main>
  );
}

export default App;
