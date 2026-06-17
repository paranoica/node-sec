import { AnalystDashboard } from "./AnalystDashboard";

// Pure client app; force a static shell so `output: export` prerenders without bailing to dynamic.
export const dynamic = "force-static";

export default function Page() {
  return <AnalystDashboard />;
}
