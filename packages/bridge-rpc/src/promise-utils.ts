export function withDelay<T>(promise: Promise<T>, delay: number): Promise<T> {
    return new Promise<T>((resolve, reject) => {
        setTimeout(() => {
            promise
                .then((result) => {
                    resolve(result);
                })
                .catch((error) => {
                    reject(error);
                });
        }, delay);
    });
}

export class TimeoutError extends Error {
    constructor(message: string) {
        super(message);
        this.name = "TimeoutError";
    }
}

export function withTimeout<T>(
    promise: Promise<T>,
    timeout: number,
): Promise<T> {
    return new Promise<T>((resolve, reject) => {
        const timer = setTimeout(() => {
            reject(new TimeoutError(`Timeout after ${timeout} ms`));
        }, timeout);

        promise
            .then((result) => {
                clearTimeout(timer);
                resolve(result);
            })
            .catch((error) => {
                clearTimeout(timer);
                reject(error);
            });
    });
}

export function delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
