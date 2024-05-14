# this is a script to build and send to raspberry pi
USERNAME=raspberry
HOST=raspberry5.local
DESTINATION_FOLDER=/home/raspberry
BUILD_ARGS=""
TARGET_PROFILE=debug

TARGET=aarch64-unknown-linux-gnu
SOURCE_FOLDER=target/$TARGET/$TARGET_PROFILE
BINARY_FILE=modbus-viewer

CROSS_ENVS=""

echo "[1/5] Building..."
eval "$CROSS_ENVS" cross build "$BUILD_ARGS" --target $TARGET

if [ "$1" = "build_only" ]; then
    exit 0
fi

echo "[1/2] Preliminary commands..."
ssh $USERNAME@$HOST "mkdir $DESTINATION_FOLDER ; cd $DESTINATION_FOLDER ; sudo -S chmod 777 $DESTINATION_FOLDER ; sudo -S rm $BINARY_FILE"
echo "[2/2] Sending the binary file..."
scp -r $SOURCE_FOLDER/$BINARY_FILE $USERNAME@$HOST:$DESTINATION_FOLDER
echo "[-/-] Program exited."
