import { createFileRoute } from "@tanstack/react-router";
import { helloListOptions } from "@/api/@tanstack/react-query.gen";
import { HelloPage } from "@/features/hello/hello-page";

export const Route = createFileRoute("/")({
  loader: ({ context }) => context.queryClient.ensureQueryData(helloListOptions()),
  component: HelloPage,
});
