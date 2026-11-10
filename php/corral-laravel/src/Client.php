<?php

namespace Corral\Laravel;

/**
 * Buffers events for the current request/command and sends them to Corral
 * as JSON lines over one unix-socket connection when the app terminates.
 * Never throws: if Corral is not listening, events are dropped silently.
 */
final class Client
{
    /** @var list<array<string, mixed>> */
    private array $events = [];

    private const MAX_EVENTS = 2000;

    public function __construct(private readonly string $socket)
    {
    }

    /** @param array<string, mixed> $data */
    public function record(string $kind, array $data): void
    {
        if (count($this->events) >= self::MAX_EVENTS) {
            return;
        }
        $this->events[] = [
            'kind' => $kind,
            'time' => microtime(true),
            'data' => $data,
        ];
    }

    public function flush(): void
    {
        if ($this->events === []) {
            return;
        }
        $request = [
            'method' => $_SERVER['REQUEST_METHOD'] ?? null,
            'host' => $_SERVER['HTTP_HOST'] ?? null,
            'uri' => $_SERVER['REQUEST_URI'] ?? null,
            'command' => PHP_SAPI === 'cli' ? implode(' ', array_slice($_SERVER['argv'] ?? [], 0, 6)) : null,
        ];
        $conn = @stream_socket_client('unix://' . $this->socket, $errno, $errstr, 0.2);
        if ($conn === false) {
            $this->events = [];
            return;
        }
        $id = bin2hex(random_bytes(6));
        foreach ($this->events as $event) {
            $event['request'] = $request;
            $event['request_id'] = $id;
            $line = json_encode($event, JSON_INVALID_UTF8_SUBSTITUTE | JSON_PARTIAL_OUTPUT_ON_ERROR);
            if ($line !== false) {
                @fwrite($conn, $line . "\n");
            }
        }
        @fclose($conn);
        $this->events = [];
    }
}
