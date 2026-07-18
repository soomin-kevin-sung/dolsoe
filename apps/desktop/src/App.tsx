import MockApp from "./MockApp";
import NativeApp from "./NativeApp";

export default function App() {
  const mockMode = new URLSearchParams(window.location.search).has("state");
  return mockMode ? <MockApp /> : <NativeApp />;
}
