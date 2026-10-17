import { useMutation, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { type FormEvent, useId, useState } from "react";
import {
  helloCreateMutation,
  helloListOptions,
  helloListQueryKey,
} from "@/api/@tanstack/react-query.gen";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { describeError, ProblemError } from "@/error";
import { NAME_ERROR, parseName } from "./name";

export function HelloPage() {
  return (
    <div className="mx-auto flex max-w-md flex-col gap-4">
      <h1 className="text-xl font-semibold">Hello</h1>
      <HelloForm />
      <HelloList />
    </div>
  );
}

function HelloForm() {
  const queryClient = useQueryClient();
  const inputId = useId();
  const errorId = useId();
  const [name, setName] = useState("");
  const [localError, setLocalError] = useState<string>();
  const create = useMutation({
    ...helloCreateMutation(),
    // Like queries: a network failure or a 5xx goes to the route error component.
    throwOnError: (error) => !(error instanceof ProblemError) || error.status >= 500,
    onSuccess: () => {
      setName("");
      return queryClient.invalidateQueries({ queryKey: helloListQueryKey() });
    },
  });

  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const parsed = parseName(name);
    if (parsed === undefined) {
      setLocalError(NAME_ERROR);
      return;
    }
    setLocalError(undefined);
    create.mutate({ body: { name: parsed } });
  };

  const error = localError ?? (create.error ? serverError(create.error) : undefined);

  return (
    <form onSubmit={onSubmit} noValidate className="flex flex-col gap-2">
      <label htmlFor={inputId} className="text-sm font-medium">
        Name
      </label>
      <div className="flex gap-2">
        <Input
          id={inputId}
          name="name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          aria-invalid={error !== undefined}
          aria-describedby={error === undefined ? undefined : errorId}
        />
        <Button type="submit" disabled={create.isPending}>
          Say hello
        </Button>
      </div>
      {error !== undefined && (
        <p id={errorId} role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
    </form>
  );
}

function serverError(error: unknown): string {
  if (error instanceof ProblemError) {
    const field = error.errors?.find((e) => e.field === "name");
    if (field) return `name ${field.message}`;
  }
  return describeError(error).message;
}

function HelloList() {
  const { data } = useSuspenseQuery(helloListOptions());
  return (
    <Card>
      <CardHeader>
        <CardTitle>Recent</CardTitle>
      </CardHeader>
      <CardContent>
        {data.length === 0 ? (
          <p className="text-sm text-muted-foreground">No hellos yet.</p>
        ) : (
          <ul aria-label="Recent hellos" className="flex flex-col gap-1">
            {data.map((hello) => (
              <li key={hello.id}>{hello.name}</li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
