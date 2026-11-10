<?php

namespace Corral\Laravel;

use Illuminate\Cache\Events\CacheHit;
use Illuminate\Cache\Events\CacheMissed;
use Illuminate\Cache\Events\KeyWritten;
use Illuminate\Database\Events\QueryExecuted;
use Illuminate\Http\Client\Events\ConnectionFailed;
use Illuminate\Http\Client\Events\ResponseReceived;
use Illuminate\Log\Events\MessageLogged;
use Illuminate\Queue\Events\JobFailed;
use Illuminate\Queue\Events\JobProcessed;
use Illuminate\Support\Facades\Event;
use Illuminate\Support\Facades\View;
use Illuminate\Support\ServiceProvider;

/**
 * Active only while Corral runs: Corral's PHP prepend file sets
 * CORRAL_EVENTS_SOCKET when its socket exists. Watchers can be switched off
 * with CORRAL_WATCH=queries,http,logs (a comma list of what to keep).
 */
class CorralServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        $socket = $_SERVER['CORRAL_EVENTS_SOCKET'] ?? (getenv('CORRAL_EVENTS_SOCKET') ?: null);
        if (!is_string($socket) || $socket === '' || !file_exists($socket)) {
            return;
        }
        $client = new Client($socket);
        $this->app->instance(Client::class, $client);

        $want = array_filter(array_map('trim', explode(',', (string) ($_SERVER['CORRAL_WATCH'] ?? getenv('CORRAL_WATCH') ?: 'queries,http,logs,views,cache,jobs'))));
        $on = static fn (string $w): bool => in_array($w, $want, true);
        $minQueryMs = (float) ($_SERVER['CORRAL_MIN_QUERY_MS'] ?? getenv('CORRAL_MIN_QUERY_MS') ?: 0);

        if ($on('queries')) {
            Event::listen(QueryExecuted::class, static function (QueryExecuted $q) use ($client, $minQueryMs): void {
                if ($q->time < $minQueryMs) {
                    return;
                }
                $client->record('query', [
                    'sql' => $q->sql,
                    'bindings' => array_map(static fn ($b) => is_scalar($b) || $b === null ? $b : (string) json_encode($b), $q->bindings),
                    'time_ms' => $q->time,
                    'connection' => $q->connectionName,
                ]);
            });
        }
        if ($on('http')) {
            Event::listen(ResponseReceived::class, static function (ResponseReceived $e) use ($client): void {
                $client->record('http', [
                    'method' => $e->request->method(),
                    'url' => $e->request->url(),
                    'status' => $e->response->status(),
                    'time_ms' => ($t = $e->response->transferStats?->getTransferTime()) !== null ? round($t * 1000, 2) : null,
                ]);
            });
            Event::listen(ConnectionFailed::class, static function (ConnectionFailed $e) use ($client): void {
                $client->record('http', [
                    'method' => $e->request->method(),
                    'url' => $e->request->url(),
                    'status' => null,
                    'error' => 'connection failed',
                ]);
            });
        }
        if ($on('logs')) {
            Event::listen(MessageLogged::class, static function (MessageLogged $e) use ($client): void {
                $client->record('log', [
                    'level' => $e->level,
                    'message' => mb_substr((string) $e->message, 0, 4000),
                    'context_keys' => array_keys($e->context),
                ]);
            });
        }
        if ($on('views')) {
            View::composer('*', static function ($view) use ($client): void {
                $client->record('view', [
                    'name' => $view->getName(),
                    'path' => method_exists($view, 'getPath') ? $view->getPath() : null,
                ]);
            });
        }
        if ($on('cache')) {
            Event::listen(CacheHit::class, static fn (CacheHit $e) => $client->record('cache', ['op' => 'hit', 'key' => $e->key]));
            Event::listen(CacheMissed::class, static fn (CacheMissed $e) => $client->record('cache', ['op' => 'miss', 'key' => $e->key]));
            Event::listen(KeyWritten::class, static fn (KeyWritten $e) => $client->record('cache', ['op' => 'write', 'key' => $e->key]));
        }
        if ($on('jobs')) {
            Event::listen(JobProcessed::class, static fn (JobProcessed $e) => $client->record('job', ['name' => $e->job->resolveName(), 'status' => 'processed', 'queue' => $e->job->getQueue()]));
            Event::listen(JobFailed::class, static fn (JobFailed $e) => $client->record('job', ['name' => $e->job->resolveName(), 'status' => 'failed', 'queue' => $e->job->getQueue(), 'error' => mb_substr($e->exception->getMessage(), 0, 2000)]));
        }

        $this->app->terminating(static fn () => $client->flush());
        // Long-running workers: also flush after each job.
        Event::listen([JobProcessed::class, JobFailed::class], static fn () => $client->flush());
    }
}
